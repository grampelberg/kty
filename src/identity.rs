use chrono::{DateTime, Utc};
use kube::CustomResource;
use russh::keys::key::PublicKey;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

static FOO: &str = "bar";

// TODO: make it possible for kube-derive to consume a variable for
// group/version
#[derive(CustomResource, Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[kube(group = "kuberift.com", version = "v1alpha1", kind = "Key", namespaced)]
pub struct KeySpec {
    id: String,
    #[serde(
        serialize_with = "public_key::serialize",
        deserialize_with = "public_key::deserialize"
    )]
    #[schemars(with = "String")]
    key: PublicKey,
    expiration: DateTime<Utc>,
}

mod public_key {
    use russh::keys::key::PublicKey;
    use russh_keys::key;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(key: &PublicKey, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&key.fingerprint())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<PublicKey, D::Error>
    where
        D: Deserializer<'de>,
    {
        let key = String::deserialize(deserializer)?;

        key::parse_public_key(key.as_bytes(), None).map_err(serde::de::Error::custom)
    }
}

// TODO: add status for things like last login
#[derive(CustomResource, Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "kuberift.com",
    version = "v1alpha1",
    kind = "User",
    namespaced
)]
pub struct UserSpec {
    id: String,
    claim: String,
}
