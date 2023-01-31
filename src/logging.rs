use atty;
use eyre::{self, WrapErr};
use tracing::warn;
use tracing_subscriber::{self, prelude::*, Layer};

fn try_init_journal() -> eyre::Result<()> {
    let journal = tracing_journald::layer().wrap_err("Failed to initialize journald logger")?;
    tracing_subscriber::registry().with(journal).try_init()?;
    Ok(())
}

fn try_init_stdout_logger() -> eyre::Result<()> {
    let stdout_log = tracing_subscriber::fmt::layer()
        .compact()
        .with_writer(std::io::stdout);
    let stderr_log = tracing_subscriber::fmt::layer()
        .compact()
        .with_writer(std::io::stderr);

    tracing_subscriber::registry()
        .with(
            stderr_log
                .with_filter(tracing_subscriber::filter::LevelFilter::INFO)
                .and_then(stdout_log),
        )
        .try_init()?;

    Ok(())
}

/// Initialize the logger
pub fn init() {
    let mut err: Option<eyre::Error> = None;
    let istty = atty::is(atty::Stream::Stdin);
    if !istty {
        err = try_init_journal().err();
    }

    if istty || err.is_some() {
        try_init_stdout_logger().err();
    }

    if let Some(e) = err {
        warn!("failed to initialize journald logger: {}", e);
    }
}
