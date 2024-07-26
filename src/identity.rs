use chrono::{DateTime, Utc};
use eyre::Result;
use kube::CustomResource;
use russh::keys::key::PublicKey;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::resources::KubeID;

// TODO: make it possible for kube-derive to consume a variable for
// group/version
#[derive(CustomResource, Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[kube(group = "kuberift.com", version = "v1alpha1", kind = "Key", namespaced)]
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

#[derive(Serialize, Deserialize, Debug, Clone, JsonSchema)]
pub struct UserStatus {
    pub last_login: Option<DateTime<Utc>>,
}

impl KubeID for User {
    fn kube_id(&self) -> Result<String> {
        self.spec.id.kube_id()
    }
}
