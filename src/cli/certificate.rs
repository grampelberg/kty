use std::time::Duration;

use backon::{
    ConstantBackoff, ConstantBuilder, ExponentialBuilder, Retryable, RetryableWithContext,
};
use base64::{engine::general_purpose::URL_SAFE, prelude::*};
use cata::{output::Format, Command, Container};
use clap::Parser;
use color_eyre::{Section, SectionExt};
use eyre::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use itertools::{Itertools, Tuples};
use jsonwebtoken::{
    decode, decode_header,
    jwk::{AlgorithmParameters, JwkSet},
    DecodingKey,
};
use serde::{de::Deserializer, Deserialize};

static CLIENT_ID: &str = "kYQRVgyf2fy8e4zw7xslOmPaLVz3jIef";
static AUDIENCE: &str = "https://kuberift.com";
static OID_CONFIG_URL: &str = "https://bigtop.auth0.com/.well-known/openid-configuration";

static TOTAL_WAIT: u64 = 60 * 10;

#[derive(Deserialize, Debug)]
struct DeviceCode {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    interval: u64,
    verification_uri_complete: String,
}

#[derive(Deserialize, Debug)]
struct OpenIdConfig {
    token_endpoint: String,
    device_authorization_endpoint: String,
    jwks_uri: String,
}

#[derive(Deserialize, Debug)]
struct Token {
    access_token: String,
    id_token: String,
    scope: String,
    #[serde(deserialize_with = "into_duration")]
    expires_in: Duration,
    token_type: String,
}

#[derive(Parser, Container)]
pub struct Certificate {
    #[clap(from_global)]
    pub output: Format,
}

fn into_duration<'de, D>(deserializer: D) -> Result<Duration, D::Error>
where
    D: Deserializer<'de>,
{
    let seconds = u64::deserialize(deserializer)?;

    Ok(Duration::from_secs(seconds))
}

impl Certificate {
    async fn token(&self, url: &str, device_code: &str) -> Result<String> {
        let data = reqwest::Client::new()
            .post(url)
            .form(&[
                ("client_id", CLIENT_ID),
                ("device_code", device_code),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ])
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;

        let content: Token =
            serde_path_to_error::deserialize(&mut serde_json::Deserializer::from_str(&data))
                .with_section(move || data.header("Response:"))?;

        Ok(content.id_token)
    }
}

#[async_trait::async_trait]
impl Command for Certificate {
    async fn run(&self) -> Result<()> {
        let cfg = reqwest::Client::new()
            .get(OID_CONFIG_URL)
            .send()
            .await?
            .error_for_status()?
            .json::<OpenIdConfig>()
            .await?;

        let data = reqwest::Client::new()
            .post(cfg.device_authorization_endpoint)
            .form(&[
                ("client_id", CLIENT_ID),
                ("scope", "openid email"),
                ("audience", AUDIENCE),
            ])
            .send()
            .await?
            .error_for_status()?
            .json::<DeviceCode>()
            .await?;

        println!(
            "Visit {} to verify your identity and enter {}",
            data.verification_uri, data.user_code
        );

        open::that(data.verification_uri_complete)?;

        let spinner = ProgressBar::new_spinner();
        spinner.enable_steady_tick(Duration::from_millis(100));
        spinner.set_style(
            ProgressStyle::with_template("{spinner:.green} {msg}")
                .unwrap()
                .tick_strings(&[".  ", ".. ", "...", " ..", "  .", "   "]),
        );
        spinner.set_message("Waiting for activation...");

        let token = (|| async { self.token(&cfg.token_endpoint, data.device_code.as_str()).await })
            .retry(
                &ConstantBuilder::default()
                    .with_delay(Duration::from_secs(data.interval))
                    .with_max_times((TOTAL_WAIT / data.interval).try_into().unwrap())
                    .with_jitter(),
            )
            .when(|e| {
                matches!(e.downcast_ref::<reqwest::Error>(), Some(e) if e.status() == Some(reqwest::StatusCode::FORBIDDEN))
            })
            .await?;

        spinner.finish_with_message("Activated!");

        let jwks = reqwest::Client::new()
            .get(cfg.jwks_uri)
            .send()
            .await?
            .error_for_status()?
            .json::<JwkSet>()
            .await?;

        let header = decode_header(&token)?;

        let Some(kid) = header.kid else {
            return Err(eyre::eyre!("Malformed token"))
                .with_section(move || format!("{:#?}", header).header("Token Header"));
        };

        let Some(jwk) = jwks.find(&kid) else {
            return Err(eyre::eyre!("JWK not found for {}", kid)).with_section(move || {
                jwks.keys
                    .iter()
                    .map(|jwk| jwk.common.key_id.as_ref().unwrap())
                    .join("\n")
                    .header("Available Key IDs")
            });
        };

        let key = match &jwk.algorithm {
            AlgorithmParameters::RSA(rsa) => DecodingKey::from_rsa_components(&rsa.n, &rsa.e)?,
            _ => return Err(eyre::eyre!("Unsupported algorithm: {:?}", header.alg)),
        };

        let validation = {
            let mut validation = jsonwebtoken::Validation::new(header.alg);
            validation.set_audience(&[AUDIENCE]);
            validation.validate_exp = false;
            validation.validate_aud = false;
            validation
        };

        let decoded = decode::<serde_json::Value>(&token, &key, &validation)?;

        println!("{:#?}", decoded);

        Ok(())
    }
}
