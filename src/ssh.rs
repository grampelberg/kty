pub(crate) mod session;

use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
};

use clap::ValueEnum;
use derive_builder::Builder;
use eyre::Result;
use k8s_openapi::{
    api::core::v1::{ObjectReference, Pod, PodStatus},
    apimachinery::pkg::apis::meta::v1,
};
use kube::runtime::events::{Event, Recorder, Reporter};
use lazy_static::lazy_static;
use prometheus::{register_int_counter, IntCounter};
use russh::server::{Config, Handler, Server};
use session::{Session, SessionBuilder};
use tracing::error;

use crate::{identity::Identity, openid};

lazy_static! {
    static ref CLIENT_COUNTER: IntCounter = register_int_counter!(
        "ssh_clients_total",
        "Number of clients created by incoming connections",
    )
    .unwrap();
    static ref SESSION_ERRORS: IntCounter = register_int_counter!(
        "ssh_session_errors_total",
        "Number of errors encountered by sessions. Note that this does not include IO errors",
    )
    .unwrap();
}

#[derive(Clone, Debug, Builder)]
pub struct CurrentPod {
    pub namespace: String,
    pub name: String,
    pub uid: String,
    pub addr: IpAddr,
}

impl Default for CurrentPod {
    fn default() -> Self {
        Self {
            namespace: "default".to_string(),
            name: "unknown".to_string(),
            uid: "unknown".to_string(),
            addr: "127.0.0.1".parse().unwrap(),
        }
    }
}

impl From<CurrentPod> for Pod {
    fn from(po: CurrentPod) -> Pod {
        Pod {
            metadata: v1::ObjectMeta {
                namespace: Some(po.namespace),
                name: Some(po.name),
                uid: Some(po.uid),
                ..Default::default()
            },
            status: Some(PodStatus {
                pod_ip: Some(po.addr.to_string()),
                ..Default::default()
            }),
            ..Default::default()
        }
    }
}

#[derive(Builder)]
pub struct Controller {
    config: kube::Config,
    #[allow(dead_code)]
    #[builder(default)]
    reporter: Option<Reporter>,
    #[builder(default)]
    current: CurrentPod,
}

impl Controller {
    pub fn client(&self) -> Result<kube::Client, kube::Error> {
        kube::Client::try_from(self.config.clone())
    }

    pub fn impersonate(
        &self,
        user: String,
        groups: Vec<String>,
    ) -> Result<kube::Client, kube::Error> {
        let mut cfg = self.config.clone();
        cfg.auth_info.impersonate = Some(user);
        cfg.auth_info.impersonate_groups = (!groups.is_empty()).then_some(groups);

        kube::Client::try_from(cfg)
    }

    #[allow(dead_code)]
    pub async fn publish(&self, obj_ref: ObjectReference, ev: Event) -> Result<()> {
        if let Some(reporter) = &self.reporter {
            Recorder::new(self.client()?, reporter.clone(), obj_ref)
                .publish(ev)
                .await?;
        }

        Ok(())
    }

    pub fn current_pod(&self) -> Pod {
        self.current.clone().into()
    }
}

#[derive(Clone, Debug, PartialEq, ValueEnum, strum::VariantArray)]
pub enum Features {
    Pty,
    IngressTunnel,
    EgressTunnel,
    Sftp,
}

#[derive(Clone, Builder)]
pub struct UIServer {
    controller: Arc<Controller>,
    identity_provider: Arc<openid::Provider>,
    features: Vec<Features>,
}

impl UIServer {
    pub async fn run(&mut self, cfg: Config, addr: (String, u16)) -> Result<()> {
        self.run_on_address(Arc::new(cfg), addr).await?;

        Ok(())
    }
}

impl Server for UIServer {
    type Handler = Session;

    fn new_client(&mut self, _: Option<SocketAddr>) -> Self::Handler {
        CLIENT_COUNTER.inc();

        SessionBuilder::default()
            .controller(self.controller.clone())
            .identity_provider(self.identity_provider.clone())
            .features(self.features.clone())
            .build()
            .expect("is valid session")
    }

    fn handle_session_error(&mut self, error: <Self::Handler as Handler>::Error) {
        if let Some(russh::Error::IO(_)) = error.downcast_ref::<russh::Error>() {
            return;
        }

        SESSION_ERRORS.inc();

        error!("unhandled session error: {:#?}", error);
    }
}

#[async_trait::async_trait]
pub trait Authenticate {
    async fn authenticate(&self, ctrl: &Controller) -> Result<Option<Identity>>;
}
