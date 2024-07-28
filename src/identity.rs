pub mod key;
pub mod user;

use std::fmt::Display;

use chrono::{DateTime, Utc};
use color_eyre::{Section, SectionExt};
use derive_builder::Builder;
use eyre::{Context, Result};
use itertools::Itertools;
use kube::{
    api::{Api, ListParams, Patch, PatchParams},
    ResourceExt,
};
use serde_json::json;

use self::user::User;
use crate::{
    resources::{ApplyPatch, MANAGER},
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
