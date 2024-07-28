use std::{collections::HashMap, fmt::Display};

use chrono::{DateTime, Utc};
use color_eyre::{Section, SectionExt};
use derive_builder::Builder;
use eyre::{eyre, Context, Result};
use itertools::Itertools;
use k8s_openapi::api::core::v1::ObjectReference;
use kube::{
    api::{Api, ListParams, ObjectMeta, PartialObjectMetaExt, Patch, PatchParams},
    runtime::events::{Event, EventType, Recorder, Reporter},
    CustomResource, Resource, ResourceExt,
};
use russh::keys::key::PublicKey;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::info;

use crate::{
    resources::{AddReferences, ApplyPatch, KubeID, MANAGER},
    ssh::{Authenticate, Controller},
};
#[derive(Clone, Debug, Builder)]
pub struct Identity {
    key: String,
    claims: serde_json::Value,
    pub expiration: DateTime<Utc>,
}

impl Identity {
    pub fn id(&self) -> Result<String> {
        let Some(id) = self.claims.get(&self.key) else {
            return Err(eyre::eyre!("Claim {} not found in token", self.key))
                .section(format!("{:#?}", self.claims).header("Token Claims"));
        };

        Ok(id.as_str().unwrap().into())
    }

    pub fn sub(&self) -> &str {
        self.claims
            .get("sub")
            .expect("ID tokens must contain a sub claim")
            .as_str()
            .unwrap()
    }
}

#[async_trait::async_trait]
impl Authenticate for Identity {
    #[tracing::instrument(skip(self, ctrl))]
    async fn authenticate(&self, ctrl: &Controller) -> Result<Option<User>> {
        let id = self.id()?;

        let user_client: Api<User> = Api::default_namespaced(ctrl.client().clone());

        // TODO: this is particularly bad, move over to using a reflector and then CRD
        // field selectors once it is beta/stable (alpha 1.30).
        let Some(user) = user_client
            .list(&ListParams::default())
            .await?
            .items
            .into_iter()
            .filter(|user: &User| user.spec.id == id)
            .collect::<Vec<User>>()
            .into_iter()
            .at_most_one()?
        else {
            return Ok(None);
        };

        user_client
            .patch_status(
                &user.name_any(),
                &PatchParams::apply(MANAGER),
                &Patch::Apply(&User::patch(&json!({
                    "status": {
                        "sub": self.sub(),
                    },
                }))?),
            )
            .await
            .wrap_err("patch sub")?;

        Ok(Some(user))
    }
}

impl Display for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Ok(id) = self.id() else {
            return write!(f, "--invalid--");
        };

        write!(f, "{id}")
    }
}

// TODO: make it possible for kube-derive to consume a variable for
// group/version
#[derive(CustomResource, Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "kuberift.com",
    version = "v1alpha1",
    kind = "Key",
    namespaced,
    status = "KeyStatus"
)]
pub struct KeySpec {
    #[serde(
        serialize_with = "public_key::serialize",
        deserialize_with = "public_key::deserialize"
    )]
    #[schemars(with = "String")]
    pub key: PublicKey,
    pub expiration: DateTime<Utc>,
}

impl Key {
    pub fn expired(&self) -> bool {
        self.spec.expiration < Utc::now()
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, JsonSchema)]
pub struct KeyStatus {
    pub last_used: Option<DateTime<Utc>>,
}

impl KubeID for PublicKey {
    fn kube_id(&self) -> Result<String> {
        // TODO: This feels wrong, but fingerprints can contain invalid id characters.
        // Is there any reason this should be something else?
        self.fingerprint().kube_id()
    }
}

mod public_key {
    use russh::keys::key::PublicKey;
    use russh_keys::{parse_public_key_base64, PublicKeyBase64};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(key: &PublicKey, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(key.public_key_base64().as_str())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<PublicKey, D::Error>
    where
        D: Deserializer<'de>,
    {
        let key = String::deserialize(deserializer)?;

        parse_public_key_base64(&key).map_err(serde::de::Error::custom)
    }
}

// TODO: add status for things like last login
#[derive(CustomResource, Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "kuberift.com",
    version = "v1alpha1",
    kind = "User",
    namespaced,
    status = "UserStatus"
)]
pub struct UserSpec {
    pub id: String,
}

#[derive(Default, Serialize, Deserialize, Debug, Clone, JsonSchema)]
pub struct UserStatus {
    pub last_login: Option<DateTime<Utc>>,
    pub sub: Option<String>,
}

impl User {
    // TODO: I'm not sure this is the right place for this. While this is ~close to
    // the sub-resource pattern (and maybe that's the correct thing to do here to
    // begin with), it seems weird to be mutating a key from the user object.
    #[tracing::instrument(skip(self, ctrl, key))]
    pub async fn set_key(
        &self,
        ctrl: &Controller,
        key: &PublicKey,
        expiration: &DateTime<Utc>,
    ) -> Result<()> {
        let mut kube_key = Key::new(
            &key.kube_id()?,
            KeySpec {
                key: key.clone(),
                expiration: *expiration,
            },
        );

        kube_key.add_controller(self)?;

        // TODO: what happens if multiple uesrs want to use the same key?
        Api::<Key>::default_namespaced(ctrl.client().clone())
            .patch(
                &kube_key.name_any(),
                &PatchParams::apply(MANAGER).force(),
                &Patch::Apply(&kube_key),
            )
            .await?;

        Ok(())
    }

    #[tracing::instrument(skip(self, ctrl))]
    pub async fn login(&self, ctrl: &Controller, method: &str) -> Result<()> {
        ctrl.publish(
            self.object_ref(&()),
            Event {
                action: "Authenticated".into(),
                reason: "Login".into(),
                type_: EventType::Normal,
                note: Some(format!("method {method}")),
                secondary: None,
            },
        )
        .await?;

        Api::<User>::default_namespaced(ctrl.client().clone())
            .patch_status(
                &self.name_any(),
                &PatchParams::apply(MANAGER).force(),
                &Patch::Apply(&User::patch(&json!({
                    "status": {
                        "last_login": Some(Utc::now()),
                    }
                }))?),
            )
            .await?;

        Ok(())
    }
}

impl KubeID for User {
    fn kube_id(&self) -> Result<String> {
        self.spec.id.kube_id()
    }
}
