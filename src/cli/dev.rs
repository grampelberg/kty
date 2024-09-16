mod agent;
mod authz;
mod dashboard;
mod shell;
mod stdin;

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
    Agent(agent::Agent),
    Authz(authz::Authz),
    Dashboard(dashboard::Dashboard),
    Shell(shell::Shell),
    Stdin(stdin::Stdin),
}

impl Command for Dev {}
