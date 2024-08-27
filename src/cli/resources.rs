use cata::{Command, Container};
use clap::{Parser, Subcommand};
use color_eyre::Section;
use either::Either::{Left, Right};
use eyre::{eyre, Result};
use futures::StreamExt;
use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use kube::{
    api::{Api, DeleteParams, ResourceExt},
    Client,
};
use serde::Serialize;
use tracing::info;

#[derive(Parser, Container)]
pub struct Resources {
    #[command(subcommand)]
    command: ResourcesCmd,
}

#[derive(Subcommand, Container)]
enum ResourcesCmd {
    Apply(Apply),
    Delete(Delete),
    Manifest(Manifest),
}

impl Command for Resources {}

#[derive(Parser, Container)]
pub struct Apply {
    /// Update resources if they already exist
    #[arg(long)]
    no_update: bool,
}

#[async_trait::async_trait]
impl Command for Apply {
    async fn run(&self) -> Result<()> {
        let client: &Api<CustomResourceDefinition> = &Api::all(Client::try_default().await?);

        crate::resources::create(client, !self.no_update).await?;

        Ok(())
    }
}

#[derive(Parser, Container)]
pub struct Delete {}

#[async_trait::async_trait]
impl Command for Delete {
    #[allow(clippy::blocks_in_conditions)]
    #[tracing::instrument(err, skip(self), fields(activity = "resources.delete"))]
    async fn run(&self) -> Result<()> {
        let client: &Api<CustomResourceDefinition> = &Api::all(Client::try_default().await?);

        let errors: Vec<kube::Error> = futures::stream::iter(crate::resources::all())
            .map(|resource| async move {
                client
                    .delete(&resource.name_any(), &DeleteParams::default())
                    .await
            })
            .buffered(100)
            .inspect(|result| {
                let Ok(either) = result else { return };

                match either {
                    Left(o) => info!(name = o.name_any(), "deleted CRD"),
                    Right(status) => info!(status = format!("{status:?}"), "deletion status"),
                }
            })
            .collect::<Vec<Result<_, _>>>()
            .await
            .into_iter()
            .filter(Result::is_err)
            .map(Result::unwrap_err)
            .collect();

        if !errors.is_empty() {
            return Err(errors
                .into_iter()
                .fold(eyre!("unable to delete resources"), |acc, err| {
                    acc.error(err)
                }));
        }

        Ok(())
    }
}

#[derive(Parser, Container)]
pub struct Manifest {}

#[async_trait::async_trait]
impl Command for Manifest {
    #[allow(clippy::blocks_in_conditions)]
    #[tracing::instrument(err, skip(self), fields(activity = "resources.manifest"))]
    async fn run(&self) -> Result<()> {
        let mut serializer = serde_yaml::Serializer::new(std::io::stdout());
        for resource in crate::resources::all() {
            resource.serialize(&mut serializer)?;
        }

        Ok(())
    }
}
