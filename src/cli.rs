mod dev;
mod resources;
mod serve;
mod users;

use std::sync::{Mutex, OnceLock};

use cata::{output::Format, Command, Container};
use clap::{Parser, Subcommand};
use clap_verbosity_flag::Verbosity;
use clio::Output;
use eyre::{eyre, Result};
use tracing::metadata::LevelFilter;
use tracing_error::ErrorLayer;
use tracing_log::AsTrace;
use tracing_subscriber::{filter::EnvFilter, prelude::*};

// While tracing allows for you to get the global log filter
// (`tracing::metadata::LevelFilter::current()`), the
// `tracing_subscriber::registry::Registry` doesn't actually set it. The
// `Subscriber` interface exposes an interface to check for `enabled()` but that
// doesn't look at the individual layers of the registry. This effectively
// copies how the global LevelFilter is set and allows other things to check
// against it in a similar fashion.
pub(crate) static LEVEL: OnceLock<LevelFilter> = OnceLock::new();

#[derive(Parser, Container)]
#[command(about, version)]
pub struct Root {
    #[command(subcommand)]
    command: RootCmd,

    /// Verbosity level, pass extra v's to increase verbosity. Note that this
    /// controls the UI's verbosity in addition to the log's verbosity. Setting
    /// to debug will show debug information in the UI.
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
    Dev(dev::Dev),
    Resources(resources::Resources),
    Serve(serve::Serve),
    Users(users::Users),
}

impl Command for Root {
    fn pre_run(&self) -> Result<()> {
        if LEVEL
            .set(self.verbosity.log_level_filter().as_trace())
            .is_err()
        {
            return Err(eyre!("log level already set"));
        }

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
