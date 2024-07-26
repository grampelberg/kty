use std::{
    borrow::{Borrow, Cow},
    net::SocketAddr,
    sync::Arc,
};

use chrono::{DateTime, Duration, Utc};
use eyre::{eyre, Report, Result};
use fast_qr::QRBuilder;
use kube::{
    api::{Api, ListParams, Patch, PatchParams},
    Resource, ResourceExt,
};
use regex::Regex;
use russh::{
    keys::key::PublicKey,
    server::{self, Auth, Config, Handler, Response, Server},
    Channel, Disconnect, MethodSet,
};
use ssh_encoding::Encode;
use tracing::{error, info};

use crate::{
    identity, openid,
    resources::{GetOwners, KubeID, Owner},
};

#[derive(Debug)]
pub enum Error {
    Auth(String),
    Config(String),
}

impl std::error::Error for Error {}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::Auth(id) => write!(f, "User not found: {id}"),
            Error::Config(err) => write!(f, "Config error: {err}"),
        }
    }
}

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

fn user_response(error: Report) -> Result<Auth> {
    let error = match error.downcast::<Error>() {
        Err(err) => return Err(err),
        Ok(err) => err,
    };

    let Error::Auth(id) = error else {
        return Err(error.into());
    };

    // TODO: this isn't great, rejection likely shouldn't be an event either but
    // it has negative implications. There needs to be some way to debug what's
    // happening though. Maybe a debug log level is enough?
    info!(id = id, "rejecting user");

    Ok(Auth::Reject {
        proceed_with_methods: None,
    })
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

    // TODO: this feels like it might actually be nifty to have as part of the code
    // itself via trait.
    // TODO: need to handle 429 responses and backoff.
    async fn authenticate_code(&mut self) -> Result<Auth> {
        let State::CodeSent(ref code, _) = self.state else {
            return Err(eyre!("Unexpected state: {:?}", self.state));
        };

        let token = match self.identity_provider.token(code).await {
            Ok(token) => token,
            Err(e) => return token_response(e),
        };

        // The device code is single use, once a token is fetched it no longer works.
        // The server will not disconnect on a failed auth - instead it'll let the user
        // try again (3 times by default).
        self.state.code_used();

        let user = match self.get_user(&token).await {
            Ok(user) => user,
            Err(e) => {
                return user_response(e);
            }
        };

        // The key should have been checked as part of auth_publickey which
        // should be configured to happen first on the client side. There's no real
        // value to checking the ID returned *and* the key at this point. Additionally,
        // this is an update operation because the keys expire at the same time as the
        // tokens do.
        if let State::KeyOffered(key) = &self.state {
            self.update_key(&user, key, &token.expiration()).await?;
        }

        self.state.authenticated(user);

        Ok(Auth::Accept)
    }

    async fn get_user(&self, token: &openid::Token) -> Result<identity::User> {
        let client: &kube::Client = self.kube.borrow();

        // TODO: it would be nice to get the other claims so that they can be added to
        // the identity. Should this construct a user identity and then filter via
        // comparison?
        let id = self.identity_provider.identity(token)?;

        // TODO: this is particularly bad, move over to using a reflector and then CRD
        // field selectors once it is beta/stable (alpha 1.30).
        let users: Vec<identity::User> = Api::default_namespaced(client.clone())
            .list(&ListParams::default())
            .await?
            .items
            .into_iter()
            .filter(|user: &identity::User| user.spec.id == id)
            .collect();

        match users.len() {
            0 => Err(Report::new(Error::Auth(id))),
            1 => Ok(users[0].clone()),
            x => Err(Report::new(Error::Config(format!(
                "{x} resources found with id: {id}"
            )))),
        }
    }

    async fn update_key(
        &self,
        user: &identity::User,
        key: &PublicKey,
        expiration: &DateTime<Utc>,
    ) -> Result<()> {
        let client: &kube::Client = self.kube.borrow();

        // TODO: make sure to add controller reference
        let mut kube_key = identity::Key::new(
            &key.kube_id()?,
            identity::KeySpec {
                key: key.clone(),
                expiration: *expiration,
            },
        );

        // TODO: what happens if multiple users want to use the same key? As it stands,
        // there would need to be a way to select your user based off the key offered.
        // Because the key is offered before giving the client a chance,
        // keyboard_interactive will never happen if you have a valid user + key.
        kube_key.add_owner(user)?;

        Api::<identity::Key>::default_namespaced(client.clone())
            .patch(
                &kube_key.name_any(),
                &PatchParams::apply("session.kuberift.com").force(),
                &Patch::Apply(&kube_key),
            )
            .await?;

        Ok(())
    }
}

// TODO(thomas): return valid errors back to the client.
#[async_trait::async_trait]
impl Handler for Session {
    type Error = eyre::Error;

    #[allow(clippy::blocks_in_conditions)]
    #[tracing::instrument(err, skip(self))]
    async fn auth_publickey(&mut self, user: &str, key: &PublicKey) -> Result<Auth> {
        info!("auth_publickey");

        self.state.key_offered(key);

        if let Some(user) = key.authenticate(self.kube.borrow()).await? {
            self.state.authenticated(user);

            return Ok(Auth::Accept);
        }

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
        _: Option<Response<'async_trait>>,
    ) -> Result<Auth> {
        info!("auth_keyboard_interactive");

        match self.state {
            State::Unauthenticated | State::KeyOffered(_) => self.send_code().await,
            State::CodeSent(..) => self.authenticate_code().await,
            _ => Err(eyre!("Unexpected state: {:?}", self.state)),
        }
    }

    // - issue an event on login
    // - update the user's status
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

#[async_trait::async_trait]
trait Authenticate {
    async fn authenticate(&self, client: &kube::Client) -> Result<Option<identity::User>>;
}

#[async_trait::async_trait]
impl Authenticate for PublicKey {
    // - get the user from owner references
    // - validate the expiration
    // - should there be a status field or condition for an existing key?
    async fn authenticate(&self, client: &kube::Client) -> Result<Option<identity::User>> {
        let keys: Api<identity::Key> = Api::default_namespaced(client.clone());

        let Some(key): Option<identity::Key> = keys.get_opt(&self.kube_id()?).await? else {
            return Ok(None);
        };

        identity::User::kind(&());

        let users: Vec<identity::User> = keys.get_owners(&key).await?;

        if key.expired() {
            return Ok(None);
        }

        Err(eyre!("unimplemented"))
    }
}
