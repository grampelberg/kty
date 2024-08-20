pub mod key;

use std::fmt::Display;

use eyre::Result;
use k8s_openapi::api::authorization::v1::{
    ResourceAttributes, SelfSubjectAccessReview, SelfSubjectAccessReviewSpec,
    SubjectAccessReviewStatus,
};
use kube::api::{Api, PostParams};

use crate::ssh::{Authenticate, Controller};

#[derive(Clone, Debug)]
pub struct Identity {
    pub name: String,
    pub groups: Vec<String>,
}

impl Identity {
    pub fn new(name: String, groups: Vec<String>) -> Self {
        Self { name, groups }
    }

    pub fn client(&self, ctrl: &Controller) -> Result<kube::Client, kube::Error> {
        ctrl.impersonate(self.name.clone(), self.groups.clone())
    }
}

#[async_trait::async_trait]
impl Authenticate for Identity {
    #[tracing::instrument(skip(self, ctrl))]
    async fn authenticate(&self, ctrl: &Controller) -> Result<Option<kube::Client>> {
        let client = self.client(ctrl)?;

        let access = Api::<SelfSubjectAccessReview>::all(client.clone())
            .create(
                &PostParams::default(),
                &SelfSubjectAccessReview {
                    spec: SelfSubjectAccessReviewSpec {
                        resource_attributes: Some(ResourceAttributes {
                            resource: Some("pods".to_string()),
                            verb: Some("list".to_string()),
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
            .await?;

        if let Some(SubjectAccessReviewStatus { allowed: false, .. }) = access.status {
            return Ok(None);
        }

        Ok(Some(client))
    }
}

impl Display for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}<groups: {}>", self.name, self.groups.join(", "))
    }
}

impl From<key::Key> for Identity {
    fn from(key: key::Key) -> Self {
        Self {
            name: key.spec.user,
            groups: key.spec.groups,
        }
    }
}
