use chrono::{DateTime, Utc};
use eyre::Result;
use kube::{
    api::{Api, Patch, PatchParams},
    CustomResource, ResourceExt,
};
use russh::keys::key::PublicKey;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::Identity;
use crate::{
    resources::{ApplyPatch, KubeID, MANAGER},
    ssh::{Authenticate, Controller},
};

// TODO: make it possible for kube-derive to consume a variable for
// group/version
#[allow(clippy::module_name_repetitions)]
#[derive(CustomResource, Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "kty.dev",
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
    pub user: String,
    pub groups: Vec<String>,
}

impl Key {
    pub fn from_identity(
        key: PublicKey,
        identity: &Identity,
        expiration: DateTime<Utc>,
    ) -> Result<Self> {
        Ok(Key::new(
            key.kube_id()?.as_str(),
            KeySpec {
                key,
                expiration,
                user: identity.name.clone(),
                groups: identity.groups.clone(),
            },
        ))
    }

    pub fn expired(&self) -> bool {
        self.spec.expiration < Utc::now()
    }

    #[tracing::instrument(skip_all)]
    pub async fn update(&self, client: kube::Client) -> Result<()> {
        Api::<Key>::default_namespaced(client)
            .patch(
                &self.name_any(),
                &PatchParams::apply(MANAGER).force(),
                &Patch::Apply(&self),
            )
            .await?;

        Ok(())
    }
}

#[allow(clippy::module_name_repetitions)]
#[derive(Serialize, Deserialize, Debug, Clone, JsonSchema)]
pub struct KeyStatus {
    pub last_used: DateTime<Utc>,
}

impl Default for KeyStatus {
    fn default() -> Self {
        Self {
            last_used: Utc::now(),
        }
    }
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
    #[tracing::instrument(skip_all)]
    async fn authenticate(&self, ctrl: &Controller) -> Result<Option<Identity>> {
        let keys: Api<Key> = Api::default_namespaced(ctrl.client()?);

        let Some(key): Option<Key> = keys.get_opt(&self.kube_id()?).await? else {
            return Ok(None);
        };

        if key.expired() {
            return Ok(None);
        }

        let Some(ident) = Identity::authenticate(&key.clone().into(), ctrl).await? else {
            return Ok(None);
        };

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

        Ok(Some(ident))
    }
}
