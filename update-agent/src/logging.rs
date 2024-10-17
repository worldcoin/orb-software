use std::io::IsTerminal;

use eyre::{self, WrapErr};
use tracing::warn;
use tracing_subscriber::{
    self,
    filter::{EnvFilter, LevelFilter},
    prelude::*,
    Layer,
};

const SYSLOG_IDENTIFIER: &str = "worldcoin-update-agent";

fn try_init_journal() -> eyre::Result<()> {
    let filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .from_env_lossy();

    let journal = tracing_journald::layer()
        .wrap_err("Failed to initialize journald logger")?
        .with_syslog_identifier(SYSLOG_IDENTIFIER.to_owned())
        .with_filter(filter);
    tracing_subscriber::registry().with(journal).try_init()?;
    Ok(())
}

fn try_init_stdout_logger() -> eyre::Result<()> {
    let filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .from_env_lossy();

    let stdout_log = tracing_subscriber::fmt::layer()
        .compact()
        .with_writer(std::io::stdout)
        .with_filter(filter);
    let stderr_log = tracing_subscriber::fmt::layer()
        .compact()
        .with_writer(std::io::stderr);

    tracing_subscriber::registry()
        .with(
            stderr_log
                .with_filter(LevelFilter::WARN)
                .and_then(stdout_log),
        )
        .try_init()?;

    Ok(())
}

/// Initialize the logger
pub fn init() {
    let mut err: Option<eyre::Error> = None;
    let istty = std::io::stdin().is_terminal();
    if !istty {
        err = try_init_journal().err();
    }

    if istty || err.is_some() {
        err = try_init_stdout_logger().err();
    }

    if let Some(e) = err {
        warn!("failed to initialize journald logger: {}", e);
    }
}
