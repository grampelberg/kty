use std::{
    borrow::{BorrowMut, Cow},
    str,
    sync::Arc,
};

use eyre::{eyre, Report, Result};
use fast_qr::QRBuilder;
use ratatui::{backend::WindowSize, layout::Size};
use replace_with::replace_with_or_abort;
use russh::{
    keys::key::PublicKey,
    server::{self, Auth, Response},
    ChannelId, MethodSet,
};
use tracing::info;

use crate::{
    dashboard::Dashboard,
    events::Event,
    identity::key::Key,
    io::Channel,
    openid,
    ssh::{Authenticate, Controller},
};

#[derive(Debug)]
pub enum State {
    Unauthenticated,
    KeyOffered(PublicKey),
    CodeSent(openid::DeviceCode, Option<PublicKey>),
    Authenticated(DebugClient, String),
    ChannelOpen(DebugClient),
    PtyStarted(Dashboard),
}

pub struct DebugClient(kube::Client);

impl std::fmt::Debug for DebugClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "kube::Client")
    }
}

impl AsRef<kube::Client> for DebugClient {
    fn as_ref(&self) -> &kube::Client {
        &self.0
    }
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

    fn authenticated(&mut self, client: kube::Client, method: String) {
        *self = State::Authenticated(DebugClient(client), method);
    }

    fn channel_opened(&mut self) {
        replace_with_or_abort(self, |self_| match self_ {
            State::Authenticated(client, _) => State::ChannelOpen(client),
            _ => self_,
        });
    }

    fn pty_started(&mut self, dashboard: Dashboard) {
        *self = State::PtyStarted(dashboard);
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
    pub fn new(controller: Arc<Controller>, identity_provider: Arc<openid::Provider>) -> Self {
        Self {
            controller,
            identity_provider,
            state: State::Unauthenticated,
        }
    }

    #[tracing::instrument(skip(self))]
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

    // TODO: need to handle 429 responses and backoff.
    #[tracing::instrument(skip(self))]
    async fn authenticate_code(&mut self) -> Result<Auth> {
        let (code, key) = {
            let State::CodeSent(code, key) = &self.state else {
                return Err(eyre!("Unexpected state: {:?}", self.state));
            };

            (code.clone(), key.clone())
        };

        let (id, expiration) = match self.identity_provider.identity(&code).await {
            Ok(id) => id,
            Err(e) => return token_response(e),
        };

        // The device code is single use, once a token is fetched it no longer works.
        // The server will not disconnect on a failed auth - instead it'll let the user
        // try again (3 times by default).
        self.state.code_used();

        let Some(user_client) = id.authenticate(&self.controller).await? else {
            // TODO: this isn't great, rejection likely shouldn't be an event either but
            // it has negative implications. There needs to be some way to debug what's
            // happening though. Maybe a debug log level is enough?
            return Ok(Auth::Reject {
                proceed_with_methods: None,
            });
        };

        self.state.authenticated(user_client, "openid".into());

        if let Some(user_key) = key {
            Key::from_identity(user_key, &id, expiration)?
                .update(self.controller.client()?)
                .await?;
        }

        Ok(Auth::Accept)
    }
}

// TODO(thomas): return valid errors back to the client.
#[async_trait::async_trait]
impl server::Handler for Session {
    type Error = eyre::Error;

    #[tracing::instrument(skip(self, key))]
    async fn auth_publickey(&mut self, user: &str, key: &PublicKey) -> Result<Auth> {
        self.state.key_offered(key);

        if let Some(client) = key.authenticate(&self.controller).await? {
            self.state.authenticated(client, "publickey".into());

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
        match self.state {
            State::Unauthenticated | State::KeyOffered(_) => self.send_code().await,
            State::CodeSent(..) => self.authenticate_code().await,
            _ => Err(eyre!("Unexpected state: {:?}", self.state)),
        }
    }

    // TODO: add some kind of event to log successful authentication.
    #[tracing::instrument(skip(self, _session))]
    async fn auth_succeeded(&mut self, _session: &mut server::Session) -> Result<()> {
        let State::Authenticated(_, ref method) = self.state else {
            return Err(eyre!("Unexpected state: {:?}", self.state));
        };

        info!(method, "authenticated");

        Ok(())
    }

    async fn channel_open_session(
        &mut self,
        _: russh::Channel<server::Msg>,
        _: &mut server::Session,
    ) -> Result<bool> {
        self.state.channel_opened();

        Ok(true)
    }

    #[tracing::instrument(skip(self, data))]
    async fn data(&mut self, _: ChannelId, data: &[u8], _: &mut server::Session) -> Result<()> {
        let State::PtyStarted(dashboard) = &self.state else {
            // TODO: this should probably be a debug statement.
            return Err(eyre!("Dashboard not started"));
        };

        dashboard.send(data.into())
    }

    async fn window_change_request(
        &mut self,
        _: ChannelId,
        cx: u32,
        cy: u32,
        px: u32,
        py: u32,
        _: &mut server::Session,
    ) -> Result<(), Self::Error> {
        if let State::PtyStarted(dashboard) = &self.state {
            #[allow(clippy::cast_possible_truncation)]
            dashboard.send(Event::Resize(WindowSize {
                columns_rows: Size {
                    width: cx as u16,
                    height: cy as u16,
                },
                pixels: Size {
                    width: px as u16,
                    height: py as u16,
                },
            }))?;
        };

        Ok(())
    }

    async fn channel_close(&mut self, _: ChannelId, _: &mut server::Session) -> Result<()> {
        if let State::PtyStarted(dashboard) = &mut self.state {
            dashboard.stop().await?;
        };

        Ok(())
    }

    #[tracing::instrument(skip(self, session))]
    async fn pty_request(
        &mut self,
        id: ChannelId,
        term: &str,
        cx: u32,
        cy: u32,
        px: u32,
        py: u32,
        modes: &[(russh::Pty, u32)],
        session: &mut server::Session,
    ) -> Result<()> {
        let State::ChannelOpen(user_client) = self.state.borrow_mut() else {
            return Err(eyre!("Unexpected state: {:?}", self.state));
        };

        let mut dashboard = Dashboard::new(user_client.as_ref().clone());

        dashboard.start(Channel::new(id, session.handle().clone()))?;

        #[allow(clippy::cast_possible_truncation)]
        dashboard.send(Event::Resize(WindowSize {
            columns_rows: Size {
                width: cx as u16,
                height: cy as u16,
            },
            pixels: Size {
                width: px as u16,
                height: py as u16,
            },
        }))?;

        self.state.pty_started(dashboard);

        Ok(())
    }
}
