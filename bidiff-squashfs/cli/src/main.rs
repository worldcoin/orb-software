use clap::Parser;
use color_eyre::Result;

use orb_bidiff_squashfs_cli::Args;
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let args = Args::parse();
    let cancel = CancellationToken::new();
    tokio::task::spawn(handle_ctrlc(cancel.clone()));

    let result = args.run(cancel.clone()).await;
    let telemetry_flusher = orb_telemetry::TelemetryConfig::new().init();

    telemetry_flusher.flush().await;

    result
}
async fn handle_ctrlc(cancel: CancellationToken) {
    let _guard = cancel.drop_guard();
    let _ = tokio::signal::ctrl_c().await;
}
