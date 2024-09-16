use cata::{Command, Container};
use clap::Parser;
use eyre::Result;

#[derive(Parser, Container)]
pub struct Agent {}

#[async_trait::async_trait]
impl Command for Agent {
    async fn run(&self) -> Result<()> {
        let mut agent = russh_keys::agent::client::AgentClient::connect_env()
            .await
            .unwrap();
        let mut identities = agent.request_identities().await.unwrap();
        assert_eq!(identities.len(), 1);
        let id = identities.pop().unwrap();

        tracing::info!(key = ?id, "identity");

        Ok(())
    }
}
