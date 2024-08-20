use cata::{Command, Container};
use clap::{Parser, Subcommand};
use eyre::Result;

#[derive(Parser, Container)]
pub struct Users {
    #[command(subcommand)]
    command: UsersCmd,
}

#[derive(Subcommand, Container)]
enum UsersCmd {
    Grant(Grant),
}

impl Command for Users {}

#[derive(Parser, Container)]
pub struct Grant {
    /// ID
    id: String,

    /// Roles
    #[arg(long)]
    role: Vec<String>,
}

// TODO: create *RoleBindings for the user to the roles
#[async_trait::async_trait]
impl Command for Grant {
    async fn run(&self) -> Result<()> {
        Ok(())
    }
}
