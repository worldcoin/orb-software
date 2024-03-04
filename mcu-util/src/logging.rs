use eyre::Result;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::Layer;

fn try_init_stdout_logger(loglevel: tracing::Level) -> Result<()> {
    let stdout_log = tracing_subscriber::fmt::layer()
        .compact()
        .with_writer(std::io::stdout)
        .with_filter(LevelFilter::from_level(loglevel));

    tracing_subscriber::registry().with(stdout_log).try_init()?;

    Ok(())
}

/// Initialize the logger
pub fn init(loglevel: tracing::Level) -> Result<()> {
    try_init_stdout_logger(loglevel)?;
    Ok(())
}
