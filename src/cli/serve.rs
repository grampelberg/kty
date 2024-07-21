use cata::{output::Format, Command, Container};
use clap::Parser;
use eyre::Result;

#[derive(Parser, Container)]
pub struct Serve {
    #[clap(from_global)]
    pub output: Format,
}

#[async_trait::async_trait]
impl Command for Serve {
    async fn run(&self) -> Result<()> {
        Ok(())
    }
}
