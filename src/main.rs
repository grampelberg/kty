//! # kty

mod broadcast;
#[warn(dead_code)]
mod cli;
mod dashboard;
mod events;
mod health;
mod identity;
mod io;
mod openid;
mod resources;
mod ssh;
mod widget;

use cata::execute;
use clap::Parser;
use eyre::Result;
use tokio::signal::unix::{signal, SignalKind};

use crate::cli::Root;

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::config::HookBuilder::default()
        .display_env_section(false)
        .display_location_section(false)
        .install()?;

    let root = Root::parse();
    let mut sigterm = signal(SignalKind::terminate())?;

    tokio::select! {
        _ = sigterm.recv() => Ok(()),
        result = execute(&root) => result,
    }
}
