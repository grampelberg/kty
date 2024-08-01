use chrono::{DateTime, Utc};
use eyre::Result;
use kube::{
    api::{Api, Patch, PatchParams},
    runtime::events::{Event, EventType},
    CustomResource, Resource, ResourceExt,
};
use russh::keys::key::PublicKey;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::key::{Key, KeySpec};
use crate::{
    resources::{AddReferences, ApplyPatch, KubeID, MANAGER},
    ssh::Controller,
};

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
    // TODO: I'm not sure this is the right place for this. It seems weird to be
    // mutating a key from the user object.
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

impl std::fmt::Display for User {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "User({})", self.spec.id)
    }
}
