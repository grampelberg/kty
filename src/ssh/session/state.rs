use std::str;

use russh::keys::key::PublicKey;

use crate::{identity::Identity, openid};

#[derive(Debug, strum_macros::AsRefStr)]
pub enum State {
    Unauthenticated,
    KeyOffered(PublicKey),
    CodeSent(openid::DeviceCode, Option<PublicKey>),
    InvalidIdentity(Identity, Option<PublicKey>),
    // TODO: once an authenticated state is reached, the user can really go do whatever they want.
    // For example, a dashboard and port-forwarding can happen. Instead of trying to show that as
    // states that get moved between, it feels like this should stop at authenticated and then let
    // each individual request track its own state. This'll require some extra work on the channel
    // side of things.
    Authenticated(DebugClient, String),
}

pub struct DebugClient(pub kube::Client);

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
}
