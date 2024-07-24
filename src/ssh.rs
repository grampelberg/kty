use std::{borrow::Cow, net::SocketAddr, sync::Arc};

use eyre::{eyre, Error, Result};
use fast_qr::QRBuilder;
use russh::{
    keys::key::PublicKey,
    server::{self, Auth, Config, Handler, Response, Server},
    Channel, Disconnect, MethodSet,
};
use tracing::info;

use crate::openid;

#[derive(Clone, Debug)]
pub struct UIServer {
    id: usize,
    identity_provider: Arc<openid::Provider>,
}

impl UIServer {
    pub fn new(provider: openid::Provider) -> Self {
        Self {
            id: 0,
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
    Authenticated(openid::Identity),
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

    fn authenticated(&mut self, identity: openid::Identity) {
        *self = State::Authenticated(identity);
    }
}

pub struct Session {
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

    async fn authenticate(&mut self) -> Result<Auth> {
        let State::CodeSent(ref code, _) = self.state else {
            return Err(eyre!("Unexpected state: {:?}", self.state));
        };

        match self.identity_provider.token(code).await {
            Ok(token) => {
                self.state
                    .authenticated(self.identity_provider.identity(&token)?);

                Ok(Auth::Accept)
            }
            Err(e) => {
                info!("Error validating code: {:?}", e);

                Ok(Auth::Partial {
                    name: Cow::Borrowed(""),
                    instructions: Cow::Owned(
                        "Waiting for activation, please try again.".to_string(),
                    ),
                    prompts: Cow::Owned(vec![(
                        Cow::Owned("Press Enter to continue".to_string()),
                        false,
                    )]),
                })
            }
        }
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
