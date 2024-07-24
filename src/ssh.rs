use std::{
    borrow::{Borrow, Cow},
    net::SocketAddr,
    sync::Arc,
};

use eyre::{eyre, Error, Report, Result};
use fast_qr::QRBuilder;
use kube::api::Api;
use russh::{
    keys::key::PublicKey,
    server::{self, Auth, Config, Handler, Response, Server},
    Channel, Disconnect, MethodSet,
};
use tracing::{error, info};

use crate::{identity, openid};

#[derive(Clone)]
pub struct UIServer {
    id: usize,
    kube: Arc<kube::Client>,
    identity_provider: Arc<openid::Provider>,
}

impl UIServer {
    pub fn new(kube: kube::Client, provider: openid::Provider) -> Self {
        Self {
            id: 0,
            kube: Arc::new(kube),
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
            kube: self.kube.clone(),
            identity_provider: self.identity_provider.clone(),
            state: State::Unauthenticated,
        }
    }
}

#[derive(Debug)]
pub enum State {
    Unauthenticated,
    KeyOffered(PublicKey),
    CodeSent(openid::DeviceCode, Option<PublicKey>),
    Authenticated(identity::User),
}

impl State {
    fn reset(&mut self) {
        *self = State::Unauthenticated;
    }

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

    fn authenticated(&mut self, user: identity::User) {
        *self = State::Authenticated(user);
    }
}

pub struct Session {
    kube: Arc<kube::Client>,
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

    fn token_response(&self, error: Report) -> Result<Auth> {
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

    fn user_response(&self, error: Report) -> Result<Auth> {
        let kube_error = match error.downcast::<kube::Error>() {
            Err(err) => return Err(err),
            Ok(err) => err,
        };

        let kube::Error::Api(resp) = kube_error else {
            return Err(kube_error.into());
        };

        if resp.code == reqwest::StatusCode::NOT_FOUND {
            info!("user not found");

            return Ok(Auth::Reject {
                proceed_with_methods: None,
            });
        }

        Err(resp.into())
    }

    async fn authenticate(&mut self) -> Result<Auth> {
        let State::CodeSent(ref code, _) = self.state else {
            return Err(eyre!("Unexpected state: {:?}", self.state));
        };

        let token = match self.identity_provider.token(code).await {
            Ok(token) => token,
            Err(e) => return self.token_response(e),
        };

        // The device code is single use, once a token is fetched it no longer works.
        // The server will not disconnect on a failed auth - instead it'll let the user
        // try again (3 times by default).
        self.state.code_used();

        let user = match self.get_user(&token).await {
            Ok(user) => user,
            Err(e) => {
                return self.user_response(e);
            }
        };

        self.state.authenticated(user);

        Ok(Auth::Accept)
    }

    async fn get_user(&self, token: &openid::Token) -> Result<identity::User> {
        let client: &kube::Client = self.kube.borrow();

        let user: identity::User = Api::default_namespaced(client.clone())
            .get(&self.identity_provider.identity(token)?)
            .await?;

        Ok(user)
    }
}

// TODO(thomas): return valid errors back to the client.
#[async_trait::async_trait]
impl Handler for Session {
    type Error = Error;

    #[tracing::instrument(skip(self))]
    async fn auth_publickey_offered(&mut self, user: &str, key: &PublicKey) -> Result<Auth> {
        info!("auth_publickey_offered");

        self.state.key_offered(key);

        Ok(Auth::Reject {
            proceed_with_methods: Some(MethodSet::KEYBOARD_INTERACTIVE),
        })
    }

    #[allow(clippy::blocks_in_conditions)]
    #[tracing::instrument(err, skip(self))]
    async fn auth_keyboard_interactive(
        &mut self,
        user: &str,
        submethods: &str,
        response: Option<Response<'async_trait>>,
    ) -> Result<Auth> {
        info!("auth_keyboard_interactive");

        match self.state {
            State::Unauthenticated | State::KeyOffered(_) => self.send_code().await,
            State::CodeSent(..) => self.authenticate().await,
            _ => Err(eyre!("Unexpected state: {:?}", self.state)),
        }
    }

    #[tracing::instrument(skip(self, session))]
    async fn auth_succeeded(&mut self, session: &mut server::Session) -> Result<()> {
        info!("auth_succeeded");

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
