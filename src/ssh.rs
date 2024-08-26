pub(crate) mod session;

use std::{net::SocketAddr, sync::Arc};

use derive_builder::Builder;
use eyre::Result;
use k8s_openapi::api::core::v1::ObjectReference;
use kube::runtime::events::{Event, Recorder, Reporter};
use lazy_static::lazy_static;
use prometheus::{register_int_counter, IntCounter};
use russh::server::{Config, Handler, Server};
use session::Session;
use tracing::error;

use crate::openid;

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

#[derive(Builder)]
pub struct Controller {
    config: kube::Config,
    #[allow(dead_code)]
    reporter: Option<Reporter>,
}

impl Controller {
    pub fn new(config: kube::Config) -> Self {
        Self {
            config,
            reporter: None,
        }
    }

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
}

#[derive(Clone)]
pub struct UIServer {
    controller: Arc<Controller>,
    identity_provider: Arc<openid::Provider>,
}

impl UIServer {
    pub fn new(controller: Controller, provider: openid::Provider) -> Self {
        Self {
            controller: Arc::new(controller),
            identity_provider: Arc::new(provider),
        }
    }

    pub async fn run(&mut self, cfg: Config, addr: (String, u16)) -> Result<()> {
        self.run_on_address(Arc::new(cfg), addr).await?;

        Ok(())
    }
}

impl Server for UIServer {
    type Handler = Session;

    fn new_client(&mut self, _: Option<SocketAddr>) -> Self::Handler {
        CLIENT_COUNTER.inc();

        Session::new(self.controller.clone(), self.identity_provider.clone())
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
    async fn authenticate(&self, ctrl: &Controller) -> Result<Option<kube::Client>>;
}
