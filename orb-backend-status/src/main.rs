use clap::Parser;
use color_eyre::eyre::Result;
use orb_backend_status::args::Args;
use tokio::signal::unix::{self, SignalKind};
use tokio_util::sync::CancellationToken;
use tracing::warn;

const SYSLOG_IDENTIFIER: &str = "worldcoin-backend-status";

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let telemetry = orb_telemetry::TelemetryConfig::new()
        .with_journald(SYSLOG_IDENTIFIER)
        .init();

    let args = Args::parse();
    let shutdown_token = CancellationToken::new();

    let mut sigterm = unix::signal(SignalKind::terminate())?;
    let mut sigint = unix::signal(SignalKind::interrupt())?;
    tokio::spawn({
        let shutdown_token = shutdown_token.clone();
        async move {
            tokio::select! {
                _ = sigterm.recv() => warn!("received SIGTERM"),
                _ = sigint.recv()  => warn!("received SIGINT"),
            }
            shutdown_token.cancel();
        }
    });

    let result = orb_backend_status::run(&args, shutdown_token.clone()).await;

    telemetry.flush().await;

    result
}


