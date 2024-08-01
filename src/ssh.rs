mod session;

use std::{net::SocketAddr, sync::Arc};

use derive_builder::Builder;
use eyre::Result;
use k8s_openapi::api::core::v1::ObjectReference;
use kube::runtime::events::{Event, Recorder, Reporter};
use russh::server::{Config, Handler, Server};
use session::Session;
use tokio::io::{AsyncRead, AsyncWrite};
use tracing::error;

use crate::{identity::user::User, openid};

#[derive(Builder)]
pub struct Controller {
    client: kube::Client,
    reporter: Reporter,
}

impl Controller {
    pub fn client(&self) -> &kube::Client {
        &self.client
    }

    pub async fn publish(&self, obj_ref: ObjectReference, ev: Event) -> Result<()> {
        Recorder::new(self.client.clone(), self.reporter.clone(), obj_ref)
            .publish(ev)
            .await?;

        Ok(())
    }
}

#[derive(Clone)]
pub struct UIServer {
    id: usize,
    controller: Arc<Controller>,
    identity_provider: Arc<openid::Provider>,
}

impl UIServer {
    pub fn new(controller: Controller, provider: openid::Provider) -> Self {
        Self {
            id: 0,
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
        self.id += 1;

        Session::new(self.controller.clone(), self.identity_provider.clone())
    }

    fn handle_session_error(&mut self, error: <Self::Handler as Handler>::Error) {
        if let Some(russh::Error::IO(_)) = error.downcast_ref::<russh::Error>() {
            return;
        }

        error!("unhandled session error: {:#?}", error);
    }
}

#[async_trait::async_trait]
pub trait Authenticate {
    async fn authenticate(&self, ctrl: &Controller) -> Result<Option<User>>;
}
