use cata::{output::Format, Command, Container};
use clap::{Parser, Subcommand};
use clap_verbosity_flag::Verbosity;
use eyre::Result;
use tracing_error::ErrorLayer;
use tracing_log::AsTrace;
use tracing_subscriber::{filter::EnvFilter, prelude::*};

use crate::cli::{certificate, resources, serve, users};

#[derive(Parser, Container)]
pub struct Root {
    #[command(subcommand)]
    command: RootCmd,

    /// Verbosity level, pass extra v's to increase verbosity
    #[command(flatten)]
    verbosity: Verbosity,

    /// Output format
    #[arg(short, long, value_enum, default_value_t = Format::Pretty, global = true)]
    pub output: Format,
}

#[derive(Subcommand, Container)]
enum RootCmd {
    Certificate(certificate::Certificate),
    Resources(resources::Resources),
    Serve(serve::Serve),
    Users(users::Users),
}

impl Command for Root {
    fn pre_run(&self) -> Result<()> {
        let filter = EnvFilter::builder()
            .with_default_directive(self.verbosity.log_level_filter().as_trace().into())
            .from_env_lossy();

        // TODO: figure out how to make with_span_events(FmtSpan::CLOSE) be configurable
        let fmt = tracing_subscriber::fmt::layer()
            .pretty()
            .with_writer(std::io::stderr)
            .with_filter(filter);

        let registry = tracing_subscriber::registry()
            .with(fmt)
            .with(ErrorLayer::default());

        registry.init();

        Ok(())
    }
}
