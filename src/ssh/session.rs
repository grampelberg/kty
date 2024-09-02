mod sftp;
mod state;

use std::{
    borrow::{BorrowMut, Cow},
    collections::HashMap,
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
use tokio::{
    sync::{mpsc::UnboundedSender, Mutex},
    task::JoinSet,
};
use tracing::debug;

use crate::{
    dashboard::Dashboard,
    events::Event,
    identity::Key,
    io::Channel,
    openid,
    resources::stream::direct,
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
            window_resize,
        }
    }
    pub struct ChannelVec: IntCounter {
        "method" => {
            open_session,
            close,
            eof,
            direct_tcpip,
            forwarded_tcpip,
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
}

lazy_static! {
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
}

lazy_static! {
    static ref REQUESTS_VEC: IntCounterVec =
        register_int_counter_vec!(opts!("requests_total", "Number of requests",), &["method"])
            .unwrap();
    static ref REQUESTS: RequestVec = RequestVec::from(&REQUESTS_VEC);
}

lazy_static! {
    static ref CHANNELS_VEC: IntCounterVec = register_int_counter_vec!(
        opts!("channels_total", "Number of channel actions",),
        &["method"]
    )
    .unwrap();
    static ref CHANNELS: ChannelVec = ChannelVec::from(&CHANNELS_VEC);
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
    controller: Arc<Controller>,
    identity_provider: Arc<openid::Provider>,

    start: DateTime<Utc>,
    state: State,
    tasks: JoinSet<Result<()>>,

    // Channels are created in the `channel_open_session` method and removed when a request comes
    // in for that channel, such as a `pty_request`.
    channels: HashMap<ChannelId, russh::Channel<server::Msg>>,

    // Subsystem requests add a writer when they are created and can handle input. This allows for
    // cross-request communication - such as error reporting in the dashboard from forwarded
    // connections.
    writers: Arc<Mutex<HashMap<ChannelId, UnboundedSender<Event>>>>,
}

impl Session {
    pub fn new(controller: Arc<Controller>, identity_provider: Arc<openid::Provider>) -> Self {
        Self {
            controller,
            identity_provider,
            start: Utc::now(),
            tasks: JoinSet::new(),
            state: State::Unauthenticated,
            channels: HashMap::new(),
            writers: Arc::new(Mutex::new(HashMap::new())),
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
            State::Authenticated(..) => {
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
        CHANNELS.open_session.inc();

        self.channels.insert(channel.id(), channel);

        Ok(true)
    }

    async fn channel_close(&mut self, id: ChannelId, _: &mut server::Session) -> Result<()> {
        ACTIVE_SESSIONS.dec();
        CHANNELS.close.inc();

        if let Some(writer) = self.writers.lock().await.remove(&id) {
            writer.send(Event::Shutdown)?;
        }

        Ok(())
    }

    async fn channel_eof(&mut self, id: ChannelId, session: &mut server::Session) -> Result<()> {
        CHANNELS.eof.inc();
        session.close(id);

        Ok(())
    }

    #[tracing::instrument(skip(self, channel, session))]
    async fn channel_open_direct_tcpip(
        &mut self,
        channel: russh::Channel<server::Msg>,
        host_to_connect: &str,
        port_to_connect: u32,
        _originator_address: &str,
        _originator_port: u32,
        session: &mut server::Session,
    ) -> Result<bool, Self::Error> {
        CHANNELS.direct_tcpip.inc();
        tracing::debug!("direct");

        let id = channel.id();
        let handle = session.handle();

        let State::Authenticated(user_client, _) = self.state.borrow_mut() else {
            UNEXPECTED_STATE
                .with_label_values(&["Authenticated", self.state.as_ref()])
                .inc();
            return Err(eyre!("Unexpected state: {:?}", self.state));
        };

        let connection_string = host_to_connect.to_string();
        let writers = self.writers.clone();
        let client = user_client.as_ref().clone();
        let host = host_to_connect.to_string();

        self.tasks.spawn(async move {
            #[allow(clippy::cast_possible_truncation)]
            let result = match direct(channel, client, host.clone(), port_to_connect as u16).await {
                Ok(()) => Ok(()),
                Err(e) => {
                    let e = e
                        .wrap_err(format!("failed to open connection to {connection_string}"))
                        .wrap_err("unable to forward connection");

                    let writers = writers.lock().await;

                    for (_, writer) in writers.iter() {
                        writer.send(Event::Error(format!("Error:{e:?}")))?;
                    }

                    handle
                        .close(id)
                        .await
                        .map_err(|()| eyre!("failed closing channel"))?;

                    Err(e)
                }
            };

            result
        });

        Ok(true)
    }

    // #[tracing::instrument(skip(self, channel, session))]
    // async fn channel_open_forwarded_tcpip(
    //     &mut self,
    //     channel: russh::Channel<server::Msg>,
    //     host_to_connect: &str,
    //     port_to_connect: u32,
    //     originator_address: &str,
    //     originator_port: u32,
    //     session: &mut server::Session,
    // ) -> Result<bool, Self::Error> {
    //     tracing::info!("forward");

    //     Ok(false)
    // }

    #[tracing::instrument(skip(self, data))]
    async fn data(&mut self, _: ChannelId, data: &[u8], _: &mut server::Session) -> Result<()> {
        TOTAL_BYTES.inc_by(data.len() as u64);

        Ok(())
    }

    async fn window_change_request(
        &mut self,
        id: ChannelId,
        cx: u32,
        cy: u32,
        px: u32,
        py: u32,
        session: &mut server::Session,
    ) -> Result<(), Self::Error> {
        REQUESTS.window_resize.inc();

        let writers = self.writers.lock().await;

        let Some(writer) = writers.get(&id) else {
            return Err(eyre!("no writer found for channel: {id}"));
        };

        #[allow(clippy::cast_possible_truncation)]
        writer.send(Event::Resize(WindowSize {
            columns_rows: Size {
                width: cx as u16,
                height: cy as u16,
            },
            pixels: Size {
                width: px as u16,
                height: py as u16,
            },
        }))?;

        session.channel_success(id);

        Ok(())
    }

    #[tracing::instrument(skip(self, _modes, session))]
    async fn pty_request(
        &mut self,
        id: ChannelId,
        term: &str,
        cx: u32,
        cy: u32,
        px: u32,
        py: u32,
        _modes: &[(russh::Pty, u32)],
        session: &mut server::Session,
    ) -> Result<()> {
        REQUESTS.pty.inc();
        tracing::debug!("pty");

        let State::Authenticated(user_client, _) = self.state.borrow_mut() else {
            UNEXPECTED_STATE
                .with_label_values(&["Authenticated", self.state.as_ref()])
                .inc();
            return Err(eyre!("Unexpected state: {:?}", self.state));
        };

        let channel = self.channels.remove(&id).ok_or_else(|| {
            eyre!("channel not found: {id}").wrap_err("failed to remove channel from channels map")
        })?;

        let mut dashboard = Dashboard::new(user_client.as_ref().clone());

        let writer = dashboard.start(
            channel.into_stream(),
            Channel::new(id, session.handle().clone()),
        )?;

        #[allow(clippy::cast_possible_truncation)]
        writer.send(Event::Resize(WindowSize {
            columns_rows: Size {
                width: cx as u16,
                height: cy as u16,
            },
            pixels: Size {
                width: px as u16,
                height: py as u16,
            },
        }))?;

        self.writers.lock().await.insert(id, writer);
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
        tracing::debug!("subsystem: {name}");

        let State::Authenticated(user_client, _) = self.state.borrow_mut() else {
            UNEXPECTED_STATE
                .with_label_values(&["ChannelOpen", self.state.as_ref()])
                .inc();
            return Err(eyre!("Unexpected state: {:?}", self.state));
        };

        if name != "sftp" {
            session.channel_failure(id);

            session.disconnect(
                Disconnect::ByApplication,
                format!("unsupported subsystem: {name}").as_str(),
                "",
            );

            return Ok(());
        }

        REQUESTS.sftp.inc();

        let channel = self.channels.remove(&id).ok_or_else(|| {
            eyre!("channel not found: {id}").wrap_err("failed to remove channel from channels map")
        })?;

        let handler = sftp::Handler::new(user_client.as_ref().clone());
        russh_sftp::server::run(channel.into_stream(), handler).await;

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

        self.tasks.abort_all();
    }
}
