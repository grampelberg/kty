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
    Create(Create),
}

impl Command for Users {}

#[derive(Parser, Container)]
pub struct Create {}

#[async_trait::async_trait]
impl Command for Create {
    async fn run(&self) -> Result<()> {
        Ok(())
    }
}
