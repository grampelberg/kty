use cata::{Command, Container};
use clap::Parser;
use eyre::{eyre, Result};
use k8s_openapi::api::authorization::v1::{
    ResourceAttributes, SelfSubjectAccessReview, SelfSubjectAccessReviewSpec,
    SelfSubjectRulesReview, SelfSubjectRulesReviewSpec,
};
use kube::api::PostParams;

#[derive(Parser, Container)]
pub struct Authz {
    user: String,

    #[arg(long)]
    list: bool,

    #[arg(short, long)]
    namespace: Option<String>,
}

#[async_trait::async_trait]
impl Command for Authz {
    async fn run(&self) -> Result<()> {
        let mut cfg = kube::Config::infer().await?;
        cfg.auth_info.impersonate = Some(self.user.clone());

        let client = kube::Client::try_from(cfg)?;

        if self.list {
            let Some(namespace) = &self.namespace else {
                return Err(eyre!("namespace is required for listing"));
            };

            return list(client, namespace.clone()).await;
        }

        let reviews = kube::Api::<SelfSubjectAccessReview>::all(client);

        let result = reviews
            .create(
                &PostParams::default(),
                &SelfSubjectAccessReview {
                    spec: SelfSubjectAccessReviewSpec {
                        resource_attributes: Some(ResourceAttributes {
                            resource: Some("pods".to_string()),
                            namespace: self.namespace.clone(),
                            verb: Some("list".to_string()),
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
            .await?;

        let Some(status) = result.status else {
            return Err(eyre!("no status found"));
        };

        tracing::info!(
            "allowed: {} reason: {}",
            status.allowed,
            status.reason.unwrap_or_default()
        );

        Ok(())
    }
}

async fn list(client: kube::Client, namespace: String) -> Result<()> {
    let client = kube::Api::<SelfSubjectRulesReview>::all(client);

    let result = client
        .create(
            &PostParams::default(),
            &SelfSubjectRulesReview {
                spec: SelfSubjectRulesReviewSpec {
                    namespace: Some(namespace),
                },
                ..Default::default()
            },
        )
        .await?;

    let Some(status) = result.status else {
        return Err(eyre!("no status found"));
    };

    if let Some(err) = status.evaluation_error {
        tracing::info!("evaluation error: {}", err);
    }

    status
        .resource_rules
        .into_iter()
        .filter(|rule| {
            rule.resources.as_ref().map_or(false, |r| {
                !r.contains(&"selfsubjectrulesreviews".to_string())
                    && !r.contains(&"selfsubjectreviews".to_string())
            })
        })
        .for_each(|rule| {
            tracing::info!("rule: {:#?}", rule);
        });

    Ok(())
}
