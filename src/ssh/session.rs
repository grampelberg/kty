use std::{
    borrow::{Borrow, Cow},
    f32::consts::E,
    str,
    sync::Arc,
};

use eyre::{eyre, Report, Result};
use fast_qr::QRBuilder;
use ratatui::{
    backend::CrosstermBackend,
    widgets::{Block, Borders, Clear, Paragraph},
    Terminal,
};
use russh::{
    keys::key::PublicKey,
    server::{self, Auth, Handle, Handler, Msg, Response},
    Channel, ChannelId, Disconnect, MethodSet,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite};
use tokio_util::{io::SyncIoBridge, time::delay_queue::Key};
use tracing::info;

use crate::{
    dashboard::Dashboard,
    identity::user::User,
    openid,
    ssh::{Authenticate, Controller},
};

#[derive(Debug)]
pub enum State {
    Unauthenticated,
    KeyOffered(PublicKey),
    CodeSent(openid::DeviceCode, Option<PublicKey>),
    Authenticated(User, String),
    ChannelCreated,
    PtyStarted,
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

    fn authenticated(&mut self, user: User, method: String) {
        *self = State::Authenticated(user, method);
    }

    fn channel_created(&mut self) {
        *self = State::ChannelCreated;
    }

    fn pty_started(&mut self) {
        *self = State::PtyStarted;
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
    dashboard: Option<Dashboard>,
}

impl Session {
    pub fn new(controller: Arc<Controller>, identity_provider: Arc<openid::Provider>) -> Self {
        Self {
            controller,
            identity_provider,
            state: State::Unauthenticated,
            dashboard: None,
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

        match self.state {
            State::Unauthenticated | State::KeyOffered(_) => self.send_code().await,
            State::CodeSent(..) => self.authenticate_code().await,
            _ => Err(eyre!("Unexpected state: {:?}", self.state)),
        }
    }

    #[tracing::instrument(skip(self, _session))]
    async fn auth_succeeded(&mut self, _session: &mut server::Session) -> Result<()> {
        info!("auth_succeeded");

        let State::Authenticated(ref user, ref method) = self.state else {
            return Err(eyre!("Unexpected state: {:?}", self.state));
        };

        user.login(&self.controller, method).await?;

        Ok(())
    }

    #[tracing::instrument(skip(self, channel, session))]
    async fn channel_open_session(
        &mut self,
        channel: Channel<server::Msg>,
        session: &mut server::Session,
    ) -> Result<bool> {
        info!("channel_open_session: {}", channel.id());

        let State::Authenticated(user, _) = &self.state else {
            return Err(eyre!("Unexpected state: {:?}", self.state));
        };

        let mut channel = channel;

        self.dashboard = Some(Dashboard::new(user.clone(), &channel.into_stream()));

        self.state.channel_created();

        tokio::spawn(async move {
            loop {
                let data = channel.make_reader().read(&mut [0u8; 1024]).await.unwrap();

                info!("data: {:?}", data);
            }
        });

        Ok(true)
    }

    #[tracing::instrument(skip(self, channel, data, session))]
    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut server::Session,
    ) -> Result<()> {
        info!("data: {:?}", data);

        if data[0] == b'q' {
            return Err(eyre!("User requested disconnect"));
        }

        Ok(())
    }

    async fn extended_data(
        &mut self,
        channel: ChannelId,
        data_type: u32,
        data: &[u8],
        session: &mut server::Session,
    ) -> Result<()> {
        info!("extended_data");

        Ok(())
    }

    async fn window_change_request(
        &mut self,
        _: ChannelId,
        col_width: u32,
        row_height: u32,
        _: u32,
        _: u32,
        _: &mut server::Session,
    ) -> Result<(), Self::Error> {
        info!("window_change_request");

        Ok(())
    }

    fn adjust_window(&mut self, _: ChannelId, current: u32) -> u32 {
        info!("adjust_window");

        current
    }

    async fn channel_close(&mut self, _: ChannelId, _: &mut server::Session) -> Result<()> {
        info!("channel_close");

        Ok(())
    }

    #[tracing::instrument(skip(self, session))]
    async fn pty_request(
        &mut self,
        id: ChannelId,
        term: &str,
        width: u32,
        height: u32,
        _: u32,
        _: u32,
        modes: &[(russh::Pty, u32)],
        session: &mut server::Session,
    ) -> Result<()> {
        info!("pty_request: {}", id);

        let State::ChannelCreated = self.state else {
            return Err(eyre!("Unexpected state: {:?}", self.state));
        };

        // self.dashboard
        //     .take()
        //     .unwrap()
        //     .start(width.try_into()?, height.try_into()?)?;

        Ok(())
    }
}
