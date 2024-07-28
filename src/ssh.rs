use std::{
    borrow::{Borrow, Cow},
    net::SocketAddr,
    sync::Arc,
};

use chrono::Utc;
use derive_builder::Builder;
use eyre::{eyre, Report, Result};
use fast_qr::QRBuilder;
use itertools::Itertools;
use k8s_openapi::api::core::v1::ObjectReference;
use kube::{
    api::{Api, Patch, PatchParams},
    runtime::events::{Event, Recorder, Reporter},
    ResourceExt,
};
use russh::{
    keys::key::PublicKey,
    server::{self, Auth, Config, Handler, Response, Server},
    Channel, Disconnect, MethodSet,
};
use serde_json::json;
use tracing::{error, info};
use tracing_error::{InstrumentResult, TracedError};

use crate::{
    identity, openid,
    resources::{ApplyPatch, GetOwners, KubeID, MANAGER},
};

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

    fn new_client(&mut self, _: Option<SocketAddr>) -> Session {
        self.id += 1;

        Session {
            controller: self.controller.clone(),
            identity_provider: self.identity_provider.clone(),
            state: State::Unauthenticated,
        }
    }

    fn handle_session_error(&mut self, error: <Self::Handler as Handler>::Error) {
        error!("unhandled session error: {error:?}");
    }
}

#[derive(Debug, Clone)]
pub enum State {
    Unauthenticated,
    KeyOffered(PublicKey),
    CodeSent(openid::DeviceCode, Option<PublicKey>),
    Authenticated(identity::User, String),
}

impl State {
    fn key_offered(&mut self, key: &PublicKey) {
        *self = State::KeyOffered(key.clone());
    }

    fn code_sent(&mut self, code: &openid::DeviceCode) {
        let key = match self {
            State::KeyOffered(key) => Some(key.clone()),
            _ => None,
        };

        *self = State::CodeSent(code.clone(), key);
    }

    fn code_used(&mut self) {
        let State::CodeSent(_, key) = self else {
            *self = State::Unauthenticated;

            return;
        };

        match key {
            Some(key) => {
                *self = State::KeyOffered(key.clone());
            }
            None => {
                *self = State::Unauthenticated;
            }
        }
    }

    fn authenticated(&mut self, user: identity::User, method: String) {
        *self = State::Authenticated(user, method);
    }
}

fn token_response(error: Report) -> Result<Auth> {
    let http_error = match error.downcast::<reqwest::Error>() {
        Err(err) => return Err(err),
        Ok(err) => err,
    };

    let Some(code) = http_error.status() else {
        return Err(http_error.into());
    };

    if code == reqwest::StatusCode::FORBIDDEN {
        info!("code not yet validated");

        return Ok(Auth::Partial {
            name: Cow::Borrowed(""),
            instructions: Cow::Owned("Waiting for activation, please try again.".to_string()),
            prompts: Cow::Owned(vec![(
                Cow::Owned("Press Enter to continue".to_string()),
                false,
            )]),
        });
    }

    Err(http_error.into())
}

pub struct Session {
    controller: Arc<Controller>,
    identity_provider: Arc<openid::Provider>,
    state: State,
}

impl Session {
    async fn send_code(&mut self) -> Result<Auth> {
        let code = self.identity_provider.code().await?;

        self.state.code_sent(&code);

        let uri = code.verification_uri_complete;

        let login_url = QRBuilder::new(uri.clone()).build().unwrap().to_str();

        let instructions =
            "\nLogin or scan the QRCode below to validate your identity:\n".to_string();

        let prompt = format!("\n{login_url}\n\n{uri}\n\nPress Enter to continue");

        Ok(Auth::Partial {
            name: Cow::Borrowed("Welcome to KubeRift"),
            instructions: Cow::Owned(instructions),
            prompts: Cow::Owned(vec![(Cow::Owned(prompt), false)]),
        })
    }

    // TODO: this feels like it might actually be nifty to have as part of the code
    // itself via trait.
    // TODO: need to handle 429 responses and backoff.
    #[tracing::instrument(skip(self))]
    async fn authenticate_code(&mut self) -> Result<Auth> {
        let State::CodeSent(code, key) = self.state.clone() else {
            return Err(eyre!("Unexpected state: {:?}", self.state));
        };

        let id = match self.identity_provider.identity(&code).await {
            Ok(id) => id,
            Err(e) => return token_response(e),
        };

        let ctrl = self.controller.borrow();

        // The device code is single use, once a token is fetched it no longer works.
        // The server will not disconnect on a failed auth - instead it'll let the user
        // try again (3 times by default).
        self.state.code_used();

        let Some(user) = id.authenticate(ctrl).await? else {
            // TODO: this isn't great, rejection likely shouldn't be an event either but
            // it has negative implications. There needs to be some way to debug what's
            // happening though. Maybe a debug log level is enough?
            info!(id = %id, "rejecting user");

            return Ok(Auth::Reject {
                proceed_with_methods: None,
            });
        };

        self.state.authenticated(user.clone(), "openid".into());

        if let Some(user_key) = key {
            user.set_key(ctrl, &user_key, &id.expiration).await?;
        }

        Ok(Auth::Accept)
    }
}

// TODO(thomas): return valid errors back to the client.
#[async_trait::async_trait]
impl Handler for Session {
    type Error = eyre::Error;

    #[tracing::instrument(skip(self, key))]
    async fn auth_publickey(&mut self, user: &str, key: &PublicKey) -> Result<Auth> {
        info!("auth_publickey");

        self.state.key_offered(key);

        if let Some(user) = key.authenticate(self.controller.borrow()).await? {
            self.state.authenticated(user, "publickey".into());

            return Ok(Auth::Accept);
        }

        Ok(Auth::Reject {
            proceed_with_methods: Some(MethodSet::KEYBOARD_INTERACTIVE),
        })
    }

    #[tracing::instrument(skip(self))]
    async fn auth_keyboard_interactive(
        &mut self,
        user: &str,
        _: &str,
        _: Option<Response<'async_trait>>,
    ) -> Result<Auth> {
        info!("auth_keyboard_interactive");

        // self.send_code().await.in_current_span()?;

        match self.state {
            State::Unauthenticated | State::KeyOffered(_) => self.send_code().await,
            State::CodeSent(..) => self.authenticate_code().await,
            _ => Err(eyre!("Unexpected state: {:?}", self.state)),
        }
    }

    #[tracing::instrument(skip(self, session))]
    async fn auth_succeeded(&mut self, session: &mut server::Session) -> Result<()> {
        info!("auth_succeeded");

        let State::Authenticated(ref user, ref method) = self.state else {
            return Err(eyre!("Unexpected state: {:?}", self.state));
        };

        user.login(&self.controller, method).await?;

        Ok(())
    }

    async fn channel_open_session(
        &mut self,
        channel: Channel<server::Msg>,
        session: &mut server::Session,
    ) -> Result<bool> {
        info!("channel_open_session");

        session.disconnect(Disconnect::ServiceNotAvailable, "unimplemented", "");

        Ok(true)
    }
}

#[async_trait::async_trait]
pub trait Authenticate {
    async fn authenticate(&self, ctrl: &Controller) -> Result<Option<identity::User>>;
}

#[async_trait::async_trait]
impl Authenticate for PublicKey {
    // - get the user from owner references
    // - validate the expiration
    // - should there be a status field or condition for an existing key?
    #[tracing::instrument(skip(self, ctrl))]
    async fn authenticate(&self, ctrl: &Controller) -> Result<Option<identity::User>> {
        let keys: Api<identity::Key> = Api::default_namespaced(ctrl.client().clone());

        let Some(key): Option<identity::Key> = keys.get_opt(&self.kube_id()?).await? else {
            return Ok(None);
        };

        if key.expired() {
            return Ok(None);
        }

        keys.patch_status(
            &key.name_any(),
            &PatchParams::apply(MANAGER).force(),
            &Patch::Apply(&identity::Key::patch(&json!({
                "status": {
                    "last_used": Some(Utc::now()),
                }
            }))?),
        )
        .await?;

        let user = keys
            .get_owners::<identity::User>(&key)
            .await?
            .into_iter()
            .at_most_one()?;

        Ok(user)
    }
}
