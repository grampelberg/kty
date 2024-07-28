//! # kuberift
mod cli;
mod identity;
mod openid;
mod resources;
mod ssh;

use cata::execute;
use clap::Parser;
use eyre::Result;

use crate::cli::Root;

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::config::HookBuilder::default()
        .display_env_section(false)
        .display_location_section(false)
        .install()?;

    execute(&Root::parse()).await
}
