mod dashboard;
mod shell;

use cata::{Command, Container};
use clap::{Parser, Subcommand};

#[derive(Parser, Container)]
/// Commands used for developing/testing functionality as individual pieces.
pub struct Dev {
    #[command(subcommand)]
    command: DevCmd,
}

#[derive(Subcommand, Container)]
enum DevCmd {
    Dashboard(dashboard::Dashboard),
    Shell(shell::Shell),
}

impl Command for Dev {}
