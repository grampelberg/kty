use cata::{Command, Container};
use clap::{Parser, Subcommand};
use eyre::Result;
use kube::{
    api::{Api, PostParams},
    Client,
};

use crate::{
    identity::user::{User, UserSpec},
    resources::KubeID,
};

#[derive(Parser, Container)]
pub struct Users {
    #[command(subcommand)]
    command: UsersCmd,
}

#[derive(Subcommand, Container)]
enum UsersCmd {
    Create(Create),
}

impl Command for Users {}

#[derive(Parser, Container)]
pub struct Create {
    /// ID
    id: String,

    /// Roles
    #[arg(long)]
    role: Vec<String>,
}

#[async_trait::async_trait]
impl Command for Create {
    async fn run(&self) -> Result<()> {
        let client: &Api<User> = &Api::default_namespaced(Client::try_default().await?);

        // TODO: output the creation result. output::Format requires Tabled which can't
        // work with kube-rs' derive. It needs to probably support Serialize as the only
        // trait bound to effectively work.
        client
            .create(
                &PostParams::default(),
                &User::new(
                    &self.id.kube_id()?,
                    UserSpec {
                        id: self.id.clone(),
                    },
                ),
            )
            .await?;

        Ok(())
    }
}
