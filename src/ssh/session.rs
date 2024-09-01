mod sftp;
mod state;

use std::{
    borrow::{BorrowMut, Cow},
    str,
    sync::Arc,
};

use chrono::{DateTime, Utc};
use eyre::{eyre, Report, Result};
use fast_qr::QRBuilder;
use lazy_static::lazy_static;
use prometheus::{
    histogram_opts, opts, register_histogram, register_int_counter, register_int_counter_vec,
    register_int_gauge, Histogram, IntCounter, IntCounterVec, IntGauge,
};
use prometheus_static_metric::make_static_metric;
use ratatui::{backend::WindowSize, layout::Size};
use russh::{
    keys::key::PublicKey,
    server::{self, Auth, Response},
    ChannelId, Disconnect, MethodSet,
};
use state::State;
use tracing::debug;

use crate::{
    dashboard::Dashboard,
    events::Event,
    identity::Key,
    io::Channel,
    openid,
    ssh::{Authenticate, Controller},
};

make_static_metric! {
    pub struct MethodVec: IntCounter {
        "method" => {
            publickey,
            interactive,
        }
    }
    pub struct ResultVec: IntCounter {
        "method" => {
            publickey,
            interactive,
        },
        "result" => {
            accept,
            partial,
            reject,
        }
    }
    pub struct CodeVec: IntCounter {
        "result" => {
            valid,
            invalid,
        }
    }
    pub struct RequestVec: IntCounter {
        "method" => {
            pty,
            sftp,
        }
    }
}

lazy_static! {
    static ref TOTAL_BYTES: IntCounter =
        register_int_counter!("bytes_received_total", "Total number of bytes received").unwrap();
    static ref TOTAL_SESSIONS: IntCounter =
        register_int_counter!("session_total", "Total number of sessions").unwrap();
    static ref ACTIVE_SESSIONS: IntGauge =
        register_int_gauge!("active_sessions", "Number of active sessions").unwrap();
    static ref SESSION_DURATION: Histogram = register_histogram!(histogram_opts!(
        "session_duration_minutes",
        "Session duration",
        vec!(0.5, 1.0, 2.0, 5.0, 10.0, 30.0, 60.0),
    ))
    .unwrap();
    static ref UNEXPECTED_STATE: IntCounterVec = register_int_counter_vec!(
        opts!(
            "unexpected_state_total",
            "Number of times an unexpected state was encountered",
        ),
        &["expected", "actual"],
    )
    .unwrap();
    static ref AUTH_ATTEMPTS_VEC: IntCounterVec = register_int_counter_vec!(
        opts!(
            "auth_attempts_total",
            "Number of authentication attempts. Note that this can seem inflated because \
             `publickey` will always be attempted first and `keyboard` will happen at least twice \
             for every success."
        ),
        &["method"]
    )
    .unwrap();
    static ref AUTH_ATTEMPTS: MethodVec = MethodVec::from(&AUTH_ATTEMPTS_VEC);
    static ref AUTH_RESULTS_VEC: IntCounterVec = register_int_counter_vec!(
        opts!(
            "auth_results_total",
            "Counter for the results of authentication attempts. Note that this can seem inflated \
             because `publickey` is always attempted first and provides a rejection before moving \
             onto other methods",
        ),
        &["method", "result"],
    )
    .unwrap();
    static ref AUTH_RESULTS: ResultVec = ResultVec::from(&AUTH_RESULTS_VEC);
    static ref AUTH_SUCEEDED: IntCounterVec = register_int_counter_vec!(
        opts!(
            "auth_succeeded_total",
            "Number of sessions that reached `auth_succeeded`."
        ),
        &["method"],
    )
    .unwrap();
    static ref CODE_GENERATED: IntCounter =
        register_int_counter!("code_generated_total", "Number of device codes generated").unwrap();
    static ref CODE_CHECKED_VEC: IntCounterVec = register_int_counter_vec!(
        opts!(
            "code_checked_total",
            "Number of times device codes have been checked",
        ),
        &["result"],
    )
    .unwrap();
    static ref CODE_CHECKED: CodeVec = CodeVec::from(&CODE_CHECKED_VEC);
    static ref WINDOW_RESIZE: IntCounter =
        register_int_counter!("window_resize_total", "Number of window resize requests").unwrap();
    static ref REQUESTS_VEC: IntCounterVec =
        register_int_counter_vec!(opts!("requests_total", "Number of requests",), &["method"])
            .unwrap();
    static ref REQUESTS: RequestVec = RequestVec::from(&REQUESTS_VEC);
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
        CODE_CHECKED.invalid.inc();

        debug!("code not yet validated");

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
    start: DateTime<Utc>,
    controller: Arc<Controller>,
    identity_provider: Arc<openid::Provider>,
    state: State,
}

impl Session {
    pub fn new(controller: Arc<Controller>, identity_provider: Arc<openid::Provider>) -> Self {
        Self {
            start: Utc::now(),
            controller,
            identity_provider,
            state: State::Unauthenticated,
        }
    }

    #[tracing::instrument(skip(self))]
    async fn send_code(&mut self) -> Result<Auth> {
        CODE_GENERATED.inc();

        let preface = if let State::InvalidIdentity(id, _) = &self.state {
            format!(
                "\nAuthenticated ID is invalid:\n- name: {}\n- groups: {}\n--------------------\n",
                id.name,
                id.groups.join(", ")
            )
        } else {
            String::new()
        };

        let code = self.identity_provider.code().await?;

        self.state.code_sent(&code);

        let uri = code.verification_uri_complete;

        let login_url = QRBuilder::new(uri.clone()).build().unwrap().to_str();

        let instructions =
            format!("{preface}\nLogin or scan the QRCode below to validate your identity:\n");

        let prompt = format!("\n{login_url}\n\n{uri}\n\nPress Enter to continue");

        AUTH_RESULTS.interactive.partial.inc();

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
                UNEXPECTED_STATE
                    .with_label_values(&["CodeSent", self.state.as_ref()])
                    .inc();
                return Err(eyre!("Unexpected state: {:?}", self.state));
            };

            (code.clone(), key.clone())
        };

        let (id, expiration) = match self.identity_provider.identity(&code).await {
            Ok(id) => id,
            Err(e) => return token_response(e),
        };

        CODE_CHECKED.valid.inc();

        // The device code is single use, once a token is fetched it no longer works.
        // The server will not disconnect on a failed auth - instead it'll let the user
        // try again (3 times by default).
        self.state.code_used();

        let Some(user_client) = id.authenticate(&self.controller).await? else {
            AUTH_RESULTS.interactive.reject.inc();

            self.state.invalid_identity(id);

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

        AUTH_RESULTS.publickey.accept.inc();

        Ok(Auth::Accept)
    }
}

// TODO(thomas): return valid errors back to the client.
#[async_trait::async_trait]
impl server::Handler for Session {
    type Error = eyre::Error;

    #[tracing::instrument(skip(self, key))]
    async fn auth_publickey(&mut self, user: &str, key: &PublicKey) -> Result<Auth> {
        AUTH_ATTEMPTS.publickey.inc();

        self.state.key_offered(key);

        if let Some(client) = key.authenticate(&self.controller).await? {
            AUTH_RESULTS.publickey.accept.inc();

            self.state.authenticated(client, "publickey".into());

            return Ok(Auth::Accept);
        }

        AUTH_RESULTS.publickey.reject.inc();

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
        AUTH_ATTEMPTS.interactive.inc();

        match self.state {
            State::Unauthenticated | State::KeyOffered(_) | State::InvalidIdentity(_, _) => {
                self.send_code().await
            }
            State::CodeSent(..) => self.authenticate_code().await,
            _ => {
                UNEXPECTED_STATE
                    .with_label_values(&[
                        "Unauthenticated | KeyOffered | CodeSent",
                        self.state.as_ref(),
                    ])
                    .inc();
                Err(eyre!("Unexpected state: {:?}", self.state))
            }
        }
    }

    // TODO: add some kind of event to log successful authentication.
    #[tracing::instrument(skip(self, _session))]
    async fn auth_succeeded(&mut self, _session: &mut server::Session) -> Result<()> {
        let State::Authenticated(_, ref method) = self.state else {
            UNEXPECTED_STATE
                .with_label_values(&["Authenticated", self.state.as_ref()])
                .inc();
            return Err(eyre!("Unexpected state: {:?}", self.state));
        };

        AUTH_SUCEEDED.with_label_values(&[method]).inc();

        debug!(method, "authenticated");

        Ok(())
    }

    async fn channel_open_session(
        &mut self,
        channel: russh::Channel<server::Msg>,
        _: &mut server::Session,
    ) -> Result<bool> {
        TOTAL_SESSIONS.inc();
        ACTIVE_SESSIONS.inc();

        self.state.channel_opened(channel);

        Ok(true)
    }

    #[tracing::instrument(skip(self, data))]
    async fn data(&mut self, _: ChannelId, data: &[u8], _: &mut server::Session) -> Result<()> {
        TOTAL_BYTES.inc_by(data.len() as u64);

        match self.state.borrow_mut() {
            State::PtyStarted(ref mut dashboard) => {
                dashboard.send(data.into())?;
            }
            // SFTP data is handled by the channel passed into russh_sftp::server::run
            State::SftpStarted => {}
            state => {
                UNEXPECTED_STATE
                    .with_label_values(&["PtyStarted", state.as_ref()])
                    .inc();

                return Err(eyre!("data received when in unexpected state: {:?}", state));
            }
        }

        Ok(())
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
        WINDOW_RESIZE.inc();

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
        ACTIVE_SESSIONS.dec();

        if let State::PtyStarted(dashboard) = &mut self.state {
            dashboard.stop().await?;
        };

        Ok(())
    }

    async fn channel_eof(&mut self, id: ChannelId, session: &mut server::Session) -> Result<()> {
        session.close(id);

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
        let State::ChannelOpen(_, user_client) = self.state.borrow_mut() else {
            UNEXPECTED_STATE
                .with_label_values(&["ChannelOpen", self.state.as_ref()])
                .inc();
            return Err(eyre!("Unexpected state: {:?}", self.state));
        };

        REQUESTS.pty.inc();

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

        session.channel_success(id);

        Ok(())
    }

    #[tracing::instrument(skip(self, session), fields(activity = "sftp"))]
    async fn subsystem_request(
        &mut self,
        id: ChannelId,
        name: &str,
        session: &mut server::Session,
    ) -> Result<(), Self::Error> {
        let State::ChannelOpen(_, _) = self.state.borrow_mut() else {
            UNEXPECTED_STATE
                .with_label_values(&["ChannelOpen", self.state.as_ref()])
                .inc();
            return Err(eyre!("Unexpected state: {:?}", self.state));
        };

        REQUESTS.sftp.inc();

        if name != "sftp" {
            session.channel_failure(id);

            session.disconnect(
                Disconnect::ByApplication,
                format!("unsupported subsystem: {name}").as_str(),
                "",
            );

            return Ok(());
        }

        let (channel, client) = self.state.take_channel_open()?;

        let handler = sftp::Handler::new(client.as_ref().clone());
        russh_sftp::server::run(channel.into_stream(), handler).await;

        self.state.sftp_started();
        session.channel_success(id);

        Ok(())
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        ACTIVE_SESSIONS.dec();

        SESSION_DURATION.observe(
            (Utc::now() - self.start)
                .to_std()
                .expect("duration in range")
                .as_secs_f64()
                / 60.0,
        );
    }
}
