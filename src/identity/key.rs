use chrono::{DateTime, Utc};
use eyre::Result;
use itertools::Itertools;
use kube::{
    api::{Api, Patch, PatchParams},
    CustomResource, ResourceExt,
};
use russh::keys::key::PublicKey;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::user::User;
use crate::{
    resources::{ApplyPatch, GetOwners, KubeID, MANAGER},
    ssh::{Authenticate, Controller},
};

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

#[async_trait::async_trait]
impl Authenticate for PublicKey {
    // - get the user from owner references
    // - validate the expiration
    // - should there be a status field or condition for an existing key?
    #[tracing::instrument(skip(self, ctrl))]
    async fn authenticate(&self, ctrl: &Controller) -> Result<Option<User>> {
        let keys: Api<Key> = Api::default_namespaced(ctrl.client().clone());

        let Some(key): Option<Key> = keys.get_opt(&self.kube_id()?).await? else {
            return Ok(None);
        };

        if key.expired() {
            return Ok(None);
        }

        keys.patch_status(
            &key.name_any(),
            &PatchParams::apply(MANAGER).force(),
            &Patch::Apply(&Key::patch(&json!({
                "status": {
                    "last_used": Some(Utc::now()),
                }
            }))?),
        )
        .await?;

        let user = keys
            .get_owners::<User>(&key)
            .await?
            .into_iter()
            .at_most_one()?;

        Ok(user)
    }
}
