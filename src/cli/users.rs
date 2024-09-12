use cata::{Command, Container};
use chrono::{DateTime, Utc};
use clap::{Parser, Subcommand};
use eyre::{eyre, Result};
use humantime::Duration;
use itertools::Itertools;
use k8s_openapi::api::rbac::v1::{ClusterRoleBinding, RoleRef, Subject};
use kube::{
    api::{Api, ObjectMeta, PostParams},
    ResourceExt,
};
use russh_keys::{key::PublicKey, parse_public_key_base64};
use serde::Serialize;

use crate::{
    identity::{key, Identity},
    resources::KubeID,
    ssh::{Authenticate, ControllerBuilder},
};

#[derive(Parser, Container)]
pub struct Users {
    #[command(subcommand)]
    command: UsersCmd,
}

#[derive(Subcommand, Container)]
enum UsersCmd {
    Check(Check),
    Grant(Grant),
    Key(Key),
}

impl Command for Users {}

/// Check if the user has access to the cluster.
#[derive(Parser, Container)]
pub struct Check {
    id: String,

    #[arg(long)]
    groups: Vec<String>,
}

#[async_trait::async_trait]
impl Command for Check {
    #[allow(clippy::blocks_in_conditions)]
    #[tracing::instrument(err, skip(self), fields(activity = "users.check"))]
    async fn run(&self) -> Result<()> {
        let identity = Identity::new(self.id.clone(), self.groups.clone());

        let ctrl = ControllerBuilder::default()
            .config(kube::Config::infer().await?)
            .build()?;

        if identity.authenticate(&ctrl).await?.is_some() {
            println!("{identity} has access");
        } else {
            return Err(eyre!("{identity} does not have access"));
        }

        Ok(())
    }
}

#[derive(Serialize)]
struct GrantOutput {
    id: String,
    role: String,
    binding: String,
    created: DateTime<Utc>,
}

/// Grant a role to a user by the provided ID. This will create a
/// `ClusterRoleBinding` that is named `kty-<id>`. If you would like to be
/// more granular, check out `kubectl create rolebinding` instead.
#[derive(Parser, Container)]
pub struct Grant {
    /// `ClusterRole` to grant. The `kty-ro` role is a good option if
    /// you're trying things out.
    role: String,

    /// ID of the user to grant the role to. This will map to how you've
    /// configured the openid provider. By default, it is `email`.
    id: String,

    /// Output the role binding instead of applying it.
    #[arg(short, long)]
    output: Option<Output>,
}

#[derive(clap::ValueEnum, Clone, Default)]
enum Output {
    #[default]
    Yaml,
}

// TODO: use cata::output, doesn't work because ratatui and tabled disagree on
// `unicode-width` versions. The CLI can be split into its own crate at this
// point and remove the direct ratatui dependency.
#[async_trait::async_trait]
impl Command for Grant {
    #[allow(clippy::blocks_in_conditions)]
    #[tracing::instrument(err, skip(self), fields(activity = "users.grant"))]
    async fn run(&self) -> Result<()> {
        let binding = ClusterRoleBinding {
            metadata: ObjectMeta {
                name: Some(format!("kty:{}", self.id).kube_id()?),
                ..Default::default()
            },
            role_ref: RoleRef {
                kind: "ClusterRole".to_string(),
                name: self.role.clone(),
                ..Default::default()
            },
            subjects: Some(vec![Subject {
                kind: "User".to_string(),
                name: self.id.clone(),
                ..Default::default()
            }]),
        };

        if let Some(Output::Yaml) = self.output {
            println!("{}", serde_yaml::to_string(&binding)?);
            return Ok(());
        }

        Api::<ClusterRoleBinding>::all(kube::Client::try_default().await?)
            .create(&PostParams::default(), &binding)
            .await?;

        println!(
            "{}",
            serde_json::to_string_pretty(&GrantOutput {
                id: self.id.clone(),
                role: self.role.clone(),
                binding: binding.name_any(),
                created: Utc::now(),
            })?
        );

        Ok(())
    }
}

#[derive(Serialize)]
struct KeyOutput {
    id: String,
    key: String,
    expiration: DateTime<Utc>,
}

/// Allow access to the cluster for the provided ID and SSH Key. This happens
/// automatically for users logging in via. openid.
#[derive(Parser, Container)]
pub struct Key {
    id: String,

    #[arg(long)]
    groups: Vec<String>,

    /// Path to the public key file you'd like to use.
    #[arg(long)]
    path: Option<String>,

    /// Optional list of base64 encoded public keys. This would be what comes
    /// out of `~/.ssh/*.pub` files and sits between the encoding and username.
    ///
    /// ```text
    /// ssh-ed25519 <public-key> foo@bar
    /// ```
    #[arg(long)]
    keys: Vec<String>,

    #[arg(long, default_value = "1y")]
    expiration: Duration,
}

#[async_trait::async_trait]
impl Command for Key {
    #[allow(clippy::blocks_in_conditions)]
    #[tracing::instrument(err, skip(self), fields(activity = "users.key"))]
    async fn run(&self) -> Result<()> {
        let mut keys: Vec<PublicKey> = self
            .keys
            .iter()
            .map(|key| parse_public_key_base64(key))
            .try_collect()?;

        if let Some(path) = &self.path {
            keys.push(russh::keys::load_public_key(path)?);
        }

        let client = Api::<key::Key>::default_namespaced(kube::Client::try_default().await?);

        let mut out = Vec::new();

        for key in keys {
            let id = key.kube_id()?;
            let resource = key::Key::new(
                id.as_str(),
                key::KeySpec {
                    key,
                    user: self.id.clone(),
                    groups: self.groups.clone(),
                    expiration: Utc::now() + *self.expiration.as_ref(),
                },
            );

            client.create(&PostParams::default(), &resource).await?;

            out.push(KeyOutput {
                id: self.id.clone(),
                key: id,
                expiration: resource.spec.expiration,
            });
        }

        println!("{}", serde_json::to_string_pretty(&out)?);

        Ok(())
    }
}
