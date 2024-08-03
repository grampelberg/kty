mod dashboard;
mod resources;
mod serve;
mod users;

use std::sync::Mutex;

use cata::{output::Format, Command, Container};
use clap::{Parser, Subcommand};
use clap_verbosity_flag::Verbosity;
use clio::Output;
use eyre::Result;
use tracing_error::ErrorLayer;
use tracing_log::AsTrace;
use tracing_subscriber::{filter::EnvFilter, prelude::*};

#[derive(Parser, Container)]
pub struct Root {
    #[command(subcommand)]
    command: RootCmd,

    /// Verbosity level, pass extra v's to increase verbosity
    #[command(flatten)]
    verbosity: Verbosity,

    /// Log destination, defaults to stderr
    #[arg(long, default_value="--", value_parser = allow_stderr)]
    log_file: Output,

    /// Output format
    #[arg(short, long, value_enum, default_value_t = Format::Pretty, global = true)]
    pub output: Format,
}

#[derive(Subcommand, Container)]
enum RootCmd {
    Dashboard(dashboard::Dashboard),
    Resources(resources::Resources),
    Serve(serve::Serve),
    Users(users::Users),
}

impl Command for Root {
    fn pre_run(&self) -> Result<()> {
        let filter = EnvFilter::builder()
            .with_default_directive(self.verbosity.log_level_filter().as_trace().into())
            .from_env_lossy();

        let fmt = tracing_subscriber::fmt::layer()
            .pretty()
            .with_writer(Mutex::new(self.log_file.clone()))
            .with_filter(filter);

        let registry = tracing_subscriber::registry()
            .with(fmt)
            .with(ErrorLayer::default());

        registry.init();

        Ok(())
    }
}

fn allow_stderr(val: &str) -> Result<Output, clio::Error> {
    if val == "--" {
        return Ok(Output::std_err());
    }

    Output::new(val)
}
