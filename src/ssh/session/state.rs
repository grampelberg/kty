use std::str;

use eyre::{eyre, Result};
use replace_with::{replace_with_or_abort, replace_with_or_abort_and_return};
use russh::{keys::key::PublicKey, server};

use crate::{dashboard::Dashboard, identity::Identity, openid};

#[derive(Debug, strum_macros::AsRefStr)]
pub enum State {
    // Used when all the fields of a variant have been removed and the next state is pending.
    Unknown,
    Unauthenticated,
    KeyOffered(PublicKey),
    CodeSent(openid::DeviceCode, Option<PublicKey>),
    InvalidIdentity(Identity, Option<PublicKey>),
    Authenticated(DebugClient, String),
    ChannelOpen(russh::Channel<server::Msg>, DebugClient),
    PtyStarted(Dashboard),
    SftpStarted,
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
    pub fn key_offered(&mut self, key: &PublicKey) {
        *self = State::KeyOffered(key.clone());
    }

    pub fn code_sent(&mut self, code: &openid::DeviceCode) {
        let key = match self {
            State::KeyOffered(key) => Some(key.clone()),
            State::InvalidIdentity(_, key) => key.clone(),
            _ => None,
        };

        *self = State::CodeSent(code.clone(), key);
    }

    pub fn code_used(&mut self) {
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

    pub fn invalid_identity(&mut self, identity: Identity) {
        let key = match self {
            State::KeyOffered(key) => Some(key.clone()),
            _ => None,
        };

        *self = State::InvalidIdentity(identity, key);
    }

    pub fn authenticated(&mut self, client: kube::Client, method: String) {
        *self = State::Authenticated(DebugClient(client), method);
    }

    pub fn channel_opened(&mut self, channel: russh::Channel<server::Msg>) {
        replace_with_or_abort(self, |self_| match self_ {
            State::Authenticated(client, _) => State::ChannelOpen(channel, client),
            _ => self_,
        });
    }

    pub fn take_channel_open(&mut self) -> Result<(russh::Channel<server::Msg>, DebugClient)> {
        replace_with_or_abort_and_return(self, |self_| match self_ {
            State::ChannelOpen(channel, client) => (Ok((channel, client)), State::Unknown),
            _ => (Err(eyre!("channel not open")), self_),
        })
    }

    pub fn pty_started(&mut self, dashboard: Dashboard) {
        *self = State::PtyStarted(dashboard);
    }

    pub fn sftp_started(&mut self) {
        *self = State::SftpStarted;
    }
}
