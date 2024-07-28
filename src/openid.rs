use chrono::Duration;
use color_eyre::{Section, SectionExt};
use derive_builder::Builder;
use eyre::Result;
use itertools::Itertools;
use jsonwebtoken::{jwk, jwk::JwkSet};
use serde::{de::Deserializer, Deserialize};
use tracing::debug;

use crate::identity::{Identity, IdentityBuilder};

#[derive(Clone, Deserialize, Debug)]
pub struct DeviceCode {
    device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    expires_in: u64,
    interval: u64,
    pub verification_uri_complete: String,
}

#[derive(Deserialize, Debug)]
struct OauthToken {
    id_token: String,
    #[serde(deserialize_with = "into_duration")]
    expires_in: Duration,
}

fn into_duration<'de, D>(deserializer: D) -> Result<Duration, D::Error>
where
    D: Deserializer<'de>,
{
    let seconds = i64::deserialize(deserializer)?;

    Ok(Duration::seconds(seconds))
}

#[derive(Clone, Debug, Deserialize)]
pub struct Config {
    token_endpoint: String,
    device_authorization_endpoint: String,
    jwks_uri: String,
}

#[async_trait::async_trait]
impl Fetch for Config {
    type Output = Self;
}

#[async_trait::async_trait]
pub trait Fetch {
    type Output: for<'de> Deserialize<'de>;

    async fn fetch(url: &str) -> Result<Self::Output> {
        let data = reqwest::Client::new()
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;

        let content: Self::Output =
            serde_path_to_error::deserialize(&mut serde_json::Deserializer::from_str(&data))
                .with_section(move || data.header("Response:"))?;

        Ok(content)
    }
}

#[async_trait::async_trait]
impl Fetch for JwkSet {
    type Output = Self;
}

impl Config {
    pub async fn jwks(&self) -> Result<JwkSet> {
        JwkSet::fetch(&self.jwks_uri).await
    }
}

#[derive(Clone, Debug, Builder)]
pub struct Provider {
    audience: String,
    client_id: String,
    claim: String,

    config: Config,
    jwks: JwkSet,
}

impl Provider {
    pub async fn code(&self) -> Result<DeviceCode> {
        let code = reqwest::Client::new()
            .post(self.config.device_authorization_endpoint.clone())
            .form(&[
                ("client_id", self.client_id.clone()),
                ("scope", "openid email".to_string()),
                ("audience", self.audience.clone()),
            ])
            .send()
            .await?
            .error_for_status()?
            .json::<DeviceCode>()
            .await?;

        Ok(code)
    }

    async fn oauth_token(&self, code: &DeviceCode) -> Result<OauthToken> {
        let data = reqwest::Client::new()
            .post(&self.config.token_endpoint)
            .form(&[
                ("client_id", &self.client_id),
                ("device_code", &code.device_code),
                (
                    "grant_type",
                    &"urn:ietf:params:oauth:grant-type:device_code".to_string(),
                ),
            ])
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;

        let content: OauthToken =
            serde_path_to_error::deserialize(&mut serde_json::Deserializer::from_str(&data))
                .with_section(move || data.header("Response:"))?;

        Ok(content)
    }

    fn id_token(&self, token: &OauthToken) -> Result<serde_json::Value> {
        let header = jsonwebtoken::decode_header(&token.id_token)?;

        let Some(kid) = header.kid else {
            return Err(eyre::eyre!("Malformed token"))
                .with_section(move || format!("{header:#?}").header("Token Header"));
        };

        let Some(jwk) = self.jwks.find(&kid) else {
            return Err(eyre::eyre!("JWK not found for {}", kid)).with_section(move || {
                self.jwks
                    .keys
                    .iter()
                    .map(|jwk| jwk.common.key_id.as_ref().unwrap())
                    .join("\n")
                    .header("Available Key IDs")
            });
        };

        let key = match &jwk.algorithm {
            jwk::AlgorithmParameters::RSA(rsa) => {
                jsonwebtoken::DecodingKey::from_rsa_components(&rsa.n, &rsa.e)?
            }
            _ => return Err(eyre::eyre!("Unsupported algorithm: {:?}", header.alg)),
        };

        let validation = {
            let mut validation = jsonwebtoken::Validation::new(header.alg);
            validation.set_audience(&[self.audience.as_str()]);
            validation.validate_exp = false;
            validation.validate_aud = false;
            validation
        };

        let token_data =
            jsonwebtoken::decode::<serde_json::Value>(&token.id_token, &key, &validation)?;

        Ok(token_data.claims)
    }

    pub async fn identity(&self, code: &DeviceCode) -> Result<Identity> {
        let oauth_token = self.oauth_token(code).await?;
        let id_token = self.id_token(&oauth_token)?;

        Ok(IdentityBuilder::default()
            .key(self.claim.clone())
            .claims(id_token)
            .expiration(chrono::Utc::now() + oauth_token.expires_in)
            .build()?)
    }
}
