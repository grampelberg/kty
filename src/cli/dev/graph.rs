use cata::{Command, Container};
use clap::Parser;
use eyre::Result;
use k8s_openapi::api::core::v1::Pod;
use kube::{api::ListParams, Api};
use petgraph::dot::{Config, Dot};

use crate::resources::ResourceGraph;

#[derive(Parser, Container)]
pub struct Cmd {}

#[async_trait::async_trait]
impl Command for Cmd {
    async fn run(&self) -> Result<()> {
        let client = kube::Client::try_default().await?;

        let pods = Api::<Pod>::all(client.clone())
            .list(&ListParams::default())
            .await?;

        for pod in pods.items {
            let g = pod.graph(&client).await?;

            let ng = g.map(
                |_, n| {
                    format!(
                        "{}/{}",
                        n.kind
                            .as_ref()
                            .unwrap_or(&"unknown".to_string())
                            .to_lowercase(),
                        n.name.as_ref().unwrap_or(&"unknown".to_string())
                    )
                },
                |_, e| e,
            );

            println!("{:#?}", Dot::with_config(&ng, &[Config::EdgeNoLabel]));
        }

        Ok(())
    }
}
