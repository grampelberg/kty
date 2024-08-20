use cata::{output::Format, Command, Container};
use clap::{
    builder::{TypedValueParser, ValueParserFactory},
    error::ErrorKind,
    Parser,
};
use eyre::Result;
use itertools::Itertools;
use kube::{api::Api, runtime::events::Reporter, Client};
use russh::{server::Config, MethodSet};

use crate::{
    openid::{self, Fetch},
    resources,
    ssh::{self, ControllerBuilder},
};

static AUDIENCE: &str = "https://kuberift.com";
static CLIENT_ID: &str = "kYQRVgyf2fy8e4zw7xslOmPaLVz3jIef";
static OID_CONFIG_URL: &str = "https://bigtop.auth0.com/.well-known/openid-configuration";

static CONTROLLER_NAME: &str = "ssh.kuberift.com";

#[derive(Parser, Container)]
pub struct Serve {
    #[clap(from_global)]
    output: Format,

    // TODO(thomas): fetch these from the CRD
    #[clap(long, default_value = "1hr")]
    inactivity_timeout: humantime::Duration,
    #[clap(long, default_value = "")]
    key_path: String,
    #[clap(long, default_value = AUDIENCE)]
    audience: String,
    #[clap(long, default_value = CLIENT_ID)]
    client_id: String,
    #[clap(long, default_value = OID_CONFIG_URL)]
    openid_configuration: String,
    /// Claim of the `id_token` to use as the user's ID.
    #[clap(long, default_value = "email")]
    claim: String,

    #[clap(long, default_value = "127.0.0.1:2222")]
    address: ListenAddr,

    #[clap(long)]
    no_create: bool,
}

#[async_trait::async_trait]
impl Command for Serve {
    async fn run(&self) -> Result<()> {
        let cfg = kube::Config::infer().await?;

        let reporter = Reporter {
            controller: CONTROLLER_NAME.into(),
            instance: Some(hostname::get()?.to_string_lossy().into()),
        };

        let ctrl = ControllerBuilder::default()
            .config(cfg)
            .reporter(reporter.clone())
            .build()?;

        if !self.no_create {
            resources::create(&Api::all(ctrl.client()?), true).await?;
        }

        let server_cfg = Config {
            inactivity_timeout: Some(self.inactivity_timeout.into()),
            methods: MethodSet::PUBLICKEY | MethodSet::KEYBOARD_INTERACTIVE,
            // TODO(thomas): how important is this? It has a negative impact on
            // UX because public key will be first, causing users to wait for
            // the first time. Maybe there's something to do with submethods?
            auth_rejection_time: std::time::Duration::from_secs(0),
            auth_rejection_time_initial: Some(std::time::Duration::from_secs(0)),
            keys: vec![russh::keys::key::KeyPair::generate_ed25519().unwrap()],
            ..Default::default()
        };

        let cfg = openid::Config::fetch(&self.openid_configuration).await?;
        let jwks = cfg.jwks().await?;

        ssh::UIServer::new(
            ctrl,
            openid::ProviderBuilder::default()
                .audience(self.audience.clone())
                .claim(self.claim.clone())
                .client_id(self.client_id.clone())
                .config(cfg)
                .jwks(jwks)
                .build()?,
        )
        .run(server_cfg, self.address.clone().into())
        .await
    }
}

#[derive(Debug, Clone, Default)]
pub struct ListenAddr {
    ip: String,
    port: u16,
}

impl From<ListenAddr> for (String, u16) {
    fn from(addr: ListenAddr) -> Self {
        (addr.ip, addr.port)
    }
}

impl TypedValueParser for ListenAddr {
    type Value = Self;

    fn parse_ref(
        &self,
        cmd: &clap::Command,
        arg: Option<&clap::Arg>,
        value: &std::ffi::OsStr,
    ) -> Result<Self::Value, clap::Error> {
        let Some((port, ip)): Option<(&str, &str)> =
            value.to_str().unwrap().rsplitn(2, ':').collect_tuple()
        else {
            return Err(cmd.clone().error(
                ErrorKind::InvalidValue,
                if let Some(arg) = arg {
                    format!(
                        "Invalid value for {}: {} is not a valid address, expected format: ip:port",
                        arg,
                        value.to_str().unwrap()
                    )
                } else {
                    format!("{value:?} is not a valid address")
                },
            ));
        };

        Ok(Self {
            ip: ip.to_string(),
            port: port.parse().map_err(|e| {
                cmd.clone().error(
                    ErrorKind::InvalidValue,
                    format!("Invalid value for port {port}: {e}"),
                )
            })?,
        })
    }
}

impl ValueParserFactory for ListenAddr {
    type Parser = Self;

    fn value_parser() -> Self {
        Self::default()
    }
}
