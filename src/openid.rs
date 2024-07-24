use std::time::Duration;

use color_eyre::{Section, SectionExt};
use eyre::Result;
use itertools::Itertools;
use jsonwebtoken::{jwk, jwk::JwkSet};
use serde::{de::Deserializer, Deserialize};

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
pub struct Token {
    access_token: String,
    id_token: String,
    scope: String,
    #[serde(deserialize_with = "into_duration")]
    expires_in: Duration,
    token_type: String,
}

fn into_duration<'de, D>(deserializer: D) -> Result<Duration, D::Error>
where
    D: Deserializer<'de>,
{
    let seconds = u64::deserialize(deserializer)?;

    Ok(Duration::from_secs(seconds))
}

pub type Identity = serde_json::Value;

#[derive(Clone, Debug, Deserialize)]
struct Config {
    token_endpoint: String,
    device_authorization_endpoint: String,
    jwks_uri: String,
}

impl Config {
    pub async fn new(url: &str) -> Result<Self> {
        let cfg = reqwest::Client::new()
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .json::<Self>()
            .await?;

        Ok(cfg)
    }
}

#[derive(Clone, Debug)]
pub struct Provider {
    audience: String,
    config: Config,
    client_id: String,
    jwks: JwkSet,
}

impl Provider {
    pub async fn new(audience: &str, client_id: &str, config_url: &str) -> Result<Self> {
        let config = Config::new(config_url).await?;

        let jwks = reqwest::Client::new()
            .get(&config.jwks_uri)
            .send()
            .await?
            .error_for_status()?
            .json::<JwkSet>()
            .await?;

        Ok(Self {
            audience: audience.to_string(),
            config,
            client_id: client_id.to_string(),
            jwks,
        })
    }

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

    pub async fn token(&self, code: &DeviceCode) -> Result<Token> {
        let data = reqwest::Client::new()
            .post(self.config.token_endpoint.clone())
            .form(&[
                ("client_id", self.client_id.clone()),
                ("device_code", code.device_code.clone()),
                (
                    "grant_type",
                    "urn:ietf:params:oauth:grant-type:device_code".to_string(),
                ),
            ])
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;

        let content: Token =
            serde_path_to_error::deserialize(&mut serde_json::Deserializer::from_str(&data))
                .with_section(move || data.header("Response:"))?;

        Ok(content)
    }

    pub fn identity(&self, token: &Token) -> Result<Identity> {
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

        let token_data = jsonwebtoken::decode::<Identity>(&token.id_token, &key, &validation)?;

        Ok(token_data.claims)
    }
}
