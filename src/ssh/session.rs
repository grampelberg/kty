mod metrics;
mod sftp;
mod state;

use std::{borrow::Cow, collections::HashMap, str, sync::Arc};

use chrono::{DateTime, Utc};
use derive_builder::Builder;
use eyre::{eyre, Report, Result};
use fast_qr::QRBuilder;
use metrics::{
    ACTIVE_SESSIONS, AUTH_ATTEMPTS, AUTH_RESULTS, AUTH_SUCEEDED, CHANNELS, CODE_CHECKED,
    CODE_GENERATED, REQUESTS, SESSION_DURATION, TOTAL_BYTES, TOTAL_SESSIONS, UNEXPECTED_STATE,
};
use ratatui::{backend::WindowSize, layout::Size};
use russh::{
    keys::key::PublicKey,
    server::{self, Auth, Response},
    ChannelId, Disconnect, MethodSet,
};
use state::State;
use tokio::task::JoinSet;
use tracing::debug;

use super::Features;
use crate::{
    broadcast::Broadcast,
    dashboard::Dashboard,
    events::Event,
    identity::Key,
    io::Channel,
    openid,
    resources::tunnel::{self, EgressBuilder, Ingress, Tunnel, TunnelBuilder},
    ssh::{Authenticate, Controller},
};

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

#[derive(Builder)]
#[builder(pattern = "owned")]
pub struct Session {
    controller: Arc<Controller>,
    identity_provider: Arc<openid::Provider>,
    features: Vec<Features>,

    #[builder(default)]
    start: DateTime<Utc>,
    #[builder(default)]
    state: State,
    // TODO: there's nothing that actually removes tasks from this set. For anything that is
    // especially long running, probably makes sense to remove them periodically with
    // `try_join_next`.
    #[builder(default)]
    tasks: JoinSet<Result<()>>,

    // Channels are created in the `channel_open_session` method and removed when a request comes
    // in for that channel, such as a `pty_request`. Note: this is being used additionally as a way
    // to track whether a channel is open or not. If a channel has been consumed, but is still
    // open, the value will be null. See `channel_eof` for an explanation on why this matters.
    #[builder(default)]
    channels: HashMap<ChannelId, Option<russh::Channel<server::Msg>>>,

    // Subsystem requests subscribe on creation if they would like to receive cross-request
    // communication - such as error reporting in the dashboard from tunnels.
    #[builder(default)]
    broadcast: Broadcast,

    // This is a somewhat special state. With my OpenSSH client, the
    // `tcpip_forward` connection comes in before the `pty` request. This makes
    // it difficult to show that there's an open, listening egress tunnel via.
    // the normal broadcast method. If this has been set, there's a broadcast of
    // `Event::Tunnel` after the dashboard startups up, very similar to the
    // window resize event.
    #[builder(default)]
    tunnel: Option<Tunnel>,
}

impl Session {
    fn enabled(&self, feature: &Features) -> bool {
        self.features.contains(feature)
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

        let Some(ident) = id.authenticate(&self.controller).await? else {
            AUTH_RESULTS.interactive.reject.inc();

            self.state.invalid_identity(id);

            return Ok(Auth::Reject {
                proceed_with_methods: None,
            });
        };

        self.state.authenticated(ident);

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
        tracing::debug!("publickey");

        self.state.key_offered(key);

        if let Some(ident) = key.authenticate(&self.controller).await? {
            AUTH_RESULTS.publickey.accept.inc();

            self.state.authenticated(ident);

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
        tracing::debug!("keyboard-interactive");

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
        let State::Authenticated(identity) = &self.state else {
            UNEXPECTED_STATE
                .with_label_values(&["Authenticated", self.state.as_ref()])
                .inc();
            return Err(eyre!("Unexpected state: {:?}", self.state));
        };

        let Some(method) = &identity.method else {
            return Err(eyre!("unexpected identity"));
        };

        AUTH_SUCEEDED.with_label_values(&[method.as_str()]).inc();

        debug!(method, "authenticated");

        Ok(())
    }

    #[tracing::instrument(skip(self, channel))]
    async fn channel_open_session(
        &mut self,
        channel: russh::Channel<server::Msg>,
        _: &mut server::Session,
    ) -> Result<bool> {
        TOTAL_SESSIONS.inc();
        ACTIVE_SESSIONS.inc();
        CHANNELS.open_session.inc();
        tracing::debug!("open-session");

        self.channels.insert(channel.id(), Some(channel));

        Ok(true)
    }

    #[tracing::instrument(skip(self))]
    async fn channel_close(&mut self, id: ChannelId, _: &mut server::Session) -> Result<()> {
        ACTIVE_SESSIONS.dec();
        CHANNELS.close.inc();
        tracing::debug!("channel-close");

        if let Some(writer) = self.broadcast.remove(&id).await {
            writer.send(Event::Shutdown)?;
        }

        Ok(())
    }

    #[tracing::instrument(skip(self, session))]
    async fn channel_eof(&mut self, id: ChannelId, session: &mut server::Session) -> Result<()> {
        CHANNELS.eof.inc();
        tracing::debug!("channel-eof");

        // You would think that it was safe to always close a channel on an EOF.
        // Unfortunately, that's not the case. For example,
        // `tokio::io::copy_bidirectional` closes the source channel *and* the
        // destination stream down correctly. If we try to close that channel here, the
        // SSH client freaks out and exits. So, for channels that need to be closed on
        // EOF (namely SFTP), we track that a channel is still "open" but consumed by
        // placing `None` into the channels hashmap. If there's any item in there, it
        // should be removed and have the shutdown triggered.
        if self.channels.remove(&id).is_some() {
            session.close(id);
        }

        Ok(())
    }

    // There is some funkiness here around showing status in the dashboard. If two
    // requests are made in parallel and one finishes first, the `Inactive` event
    // will be sent, even though one is still active.
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
        tracing::debug!("ingress-tunnel");

        if !self.enabled(&Features::IngressTunnel) {
            session.channel_failure(channel.id());

            return Ok(false);
        }

        let State::Authenticated(identity) = &self.state else {
            UNEXPECTED_STATE
                .with_label_values(&["Authenticated", self.state.as_ref()])
                .inc();
            return Err(eyre!("Unexpected state: {:?}", self.state));
        };

        let meta = TunnelBuilder::default()
            .host(host_to_connect.to_string())
            .port(u16::try_from(port_to_connect)?)
            .kind(tunnel::Kind::Ingress)
            .lifecycle(tunnel::Lifecycle::Active)
            .build()?;

        self.broadcast.all(Event::Tunnel(Ok(meta.clone()))).await?;

        let id = channel.id();
        let handle = session.handle();
        let broadcast = self.broadcast.clone();
        let client = identity.client(&self.controller)?;

        #[allow(clippy::cast_possible_truncation)]
        let ingress = Ingress::new(host_to_connect, port_to_connect as u16)?;

        self.tasks.spawn(async move {
            let meta = meta.into_inactive();

            #[allow(clippy::cast_possible_truncation)]
            match ingress.run(client, channel).await {
                Ok(()) => {
                    broadcast.all(Event::Tunnel(Ok(meta))).await?;
                    Ok(())
                }
                Err(e) => {
                    let e = e
                        .wrap_err(format!("failed to open connection to {}", ingress.host()))
                        .wrap_err("unable to forward connection");

                    broadcast
                        .all(Event::Tunnel(Err(tunnel::Error::new(&e, meta))))
                        .await?;

                    handle
                        .close(id)
                        .await
                        .map_err(|()| eyre!("failed closing channel"))?;

                    Err(e)
                }
            }
        });

        Ok(true)
    }

    #[tracing::instrument(skip(self, data))]
    async fn data(&mut self, _: ChannelId, data: &[u8], _: &mut server::Session) -> Result<()> {
        TOTAL_BYTES.inc_by(data.len() as u64);

        Ok(())
    }

    #[tracing::instrument(skip(self, session))]
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
        tracing::debug!("window change");

        #[allow(clippy::cast_possible_truncation)]
        self.broadcast
            .send(
                &id,
                Event::Resize(WindowSize {
                    columns_rows: Size {
                        width: cx as u16,
                        height: cy as u16,
                    },
                    pixels: Size {
                        width: px as u16,
                        height: py as u16,
                    },
                }),
            )
            .await?;

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

        if !self.enabled(&Features::Pty) {
            session.channel_failure(id);

            return Ok(());
        }

        let State::Authenticated(identity) = &self.state else {
            UNEXPECTED_STATE
                .with_label_values(&["Authenticated", self.state.as_ref()])
                .inc();
            return Err(eyre!("Unexpected state: {:?}", self.state));
        };

        let Some(channel) = self.channels.remove(&id).ok_or_else(|| {
            eyre!("channel not found: {id}").wrap_err("failed to remove channel from channels map")
        })?
        else {
            return Err(eyre!("channel {id} already consumed"));
        };

        let mut dashboard = Dashboard::new(identity.client(&self.controller)?);

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

        if let Some(tunnel) = self.tunnel.take() {
            writer.send(Event::Tunnel(Ok(tunnel.clone())))?;
        }

        self.broadcast.add(id, writer).await?;
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

        let State::Authenticated(identity) = &self.state else {
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

        if !self.enabled(&Features::Sftp) {
            session.channel_failure(id);

            return Ok(());
        }

        let Some(channel) = self.channels.remove(&id).ok_or_else(|| {
            eyre!("channel not found: {id}").wrap_err("failed to remove channel from channels map")
        })?
        else {
            return Err(eyre!("channel {id} already consumed"));
        };

        self.channels.insert(id, None);

        let handler = sftp::Handler::new(identity.client(&self.controller)?);
        russh_sftp::server::run(channel.into_stream(), handler).await;

        session.channel_success(id);

        Ok(())
    }

    #[tracing::instrument(skip(self, session))]
    async fn tcpip_forward(
        &mut self,
        address: &str,
        port: &mut u32,
        session: &mut server::Session,
    ) -> Result<bool, Self::Error> {
        REQUESTS.tcpip_forward.inc();
        tracing::debug!("egress-tunnel");

        if !self.enabled(&Features::EgressTunnel) {
            return Ok(false);
        }

        let State::Authenticated(identity) = &self.state else {
            UNEXPECTED_STATE
                .with_label_values(&["ChannelOpen", self.state.as_ref()])
                .inc();
            return Err(eyre!("Unexpected state: {:?}", self.state));
        };

        let meta = TunnelBuilder::default()
            .host(address.to_string())
            .port(u16::try_from(*port)?)
            .kind(tunnel::Kind::Egress)
            .lifecycle(tunnel::Lifecycle::Listening)
            .build()?;
        self.tunnel = Some(meta.clone());

        if address == "localhost" {
            self.broadcast
                .all(Event::Tunnel(Err(tunnel::Error::new(
                    &eyre!("use -R <namespace>/<service>:<remote-port>:localhost:<local-port>")
                        .wrap_err("localhost is not allowed as a source"),
                    meta.clone(),
                ))))
                .await?;

            return Ok(false);
        }

        let handle = session.handle();
        let broadcast = self.broadcast.clone();
        #[allow(clippy::cast_possible_truncation)]
        let mut egress = EgressBuilder::default()
            .host(address)?
            .port(*port as u16)
            .identity(identity)
            .server(self.controller.server())
            .meta(meta.clone())
            .broadcast(broadcast.clone())
            .build()?;
        let client = identity.client(&self.controller)?;

        self.tasks.spawn(async move {
            match egress.run(client, handle.clone()).await {
                Ok(()) => Ok(()),
                Err(e) => {
                    tracing::error!("egress-tunnel: {:?}", e);

                    handle
                        .disconnect(
                            Disconnect::ByApplication,
                            format!("unrecoverable error, reconnect and try again: {e}"),
                            String::new(),
                        )
                        .await?;

                    Err(e)
                }
            }
        });

        Ok(true)
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
