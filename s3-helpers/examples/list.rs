use clap::Parser;
use color_eyre::{eyre::Context as _, Result};
use futures::TryStreamExt as _;
use orb_s3_helpers::{ClientExt as _, S3Uri};
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let flusher = orb_telemetry::TelemetryConfig::new().init();

    let args = Command::parse();
    let result = run(args).await;

    flusher.flush().await;

    result
}

#[derive(Debug, Parser)]
struct Command {
    /// The s3 uri prefix to sync
    #[clap(long)]
    prefix: S3Uri,
}

async fn run(args: Command) -> Result<()> {
    let client = orb_s3_helpers::client()
        .await
        .wrap_err("failed to initialize client")?;

    let mut stream = client.list_prefix(args.prefix);
    while let Some(obj) = stream.try_next().await? {
        info!("{obj:?}");
    }

    Ok(())
}
