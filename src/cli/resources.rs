use cata::{Command, Container};
use clap::{Parser, Subcommand};
use eyre::Result;
use kube::{
    api::{DeleteParams, Patch, PatchParams, ResourceExt},
    Client,
};
use serde::Serialize;

use super::namespace;
use crate::resources::{install, DynamicClient, GetGvk, MANAGER};

#[derive(Parser, Container)]
pub struct Resources {
    #[command(subcommand)]
    command: ResourcesCmd,

    /// Namespace to apply resources to, will use your default namespace if not
    /// set.
    #[arg(short, long, global = true)]
    namespace: Option<String>,
}

#[derive(Subcommand, Container)]
enum ResourcesCmd {
    Crd(Crd),
    Delete(Delete),
    Install(Install),
}

impl Command for Resources {}

#[derive(Parser, Container)]
pub struct Crd {}

#[async_trait::async_trait]
impl Command for Crd {
    #[allow(clippy::blocks_in_conditions)]
    #[tracing::instrument(err, skip(self), fields(activity = "resources.crd"))]
    async fn run(&self) -> Result<()> {
        let mut serializer = serde_yaml::Serializer::new(std::io::stdout());
        for resource in crate::resources::all() {
            resource.serialize(&mut serializer)?;
        }

        Ok(())
    }
}

#[derive(Parser, Container)]
pub struct Delete {
    #[arg(from_global)]
    namespace: Option<String>,
}

#[async_trait::async_trait]
impl Command for Delete {
    #[allow(clippy::blocks_in_conditions)]
    #[tracing::instrument(err, skip(self), fields(activity = "resources.delete"))]
    async fn run(&self) -> Result<()> {
        let client = Client::try_default().await?;

        let namespace = namespace(self.namespace.as_ref()).await?;

        let resources = install::add_patches(namespace.as_str(), install::list()?)?;

        for resource in resources {
            let gvk = resource.gvk()?;

            tracing::info!("deleting: {}/{}", gvk.kind, resource.name_any());

            resource
                .dynamic(client.clone())
                .await?
                .delete(resource.name_any().as_str(), &DeleteParams::default())
                .await?;
        }

        Ok(())
    }
}

#[derive(Parser, Container)]
pub struct Install {
    /// Don't actually apply changes, just print what would happen.
    #[arg(long)]
    dry_run: bool,

    #[arg(from_global)]
    namespace: Option<String>,
}

#[async_trait::async_trait]
impl Command for Install {
    #[allow(clippy::blocks_in_conditions)]
    #[tracing::instrument(err, skip(self), fields(activity = "resources.install"))]
    async fn run(&self) -> Result<()> {
        let namespace = namespace(self.namespace.as_ref()).await?;

        let resources = install::add_patches(namespace.as_str(), install::list()?)?;

        if self.dry_run {
            let mut serializer = serde_yaml::Serializer::new(std::io::stdout());

            for resource in resources {
                resource.serialize(&mut serializer)?;
            }

            return Ok(());
        }

        let client = Client::try_default().await?;

        for resource in resources {
            let gvk = resource.gvk()?;

            tracing::info!("creating/updating: {}/{}", gvk.kind, resource.name_any());

            resource
                .dynamic(client.clone())
                .await?
                .patch(
                    resource.name_any().as_str(),
                    &PatchParams::apply(MANAGER).force(),
                    &Patch::Apply(&resource),
                )
                .await?;
        }

        Ok(())
    }
}
