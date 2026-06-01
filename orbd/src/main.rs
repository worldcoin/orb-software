use clap::Parser;
use color_eyre::eyre::Result;
use orb_build_info::{make_build_info, BuildInfo};
use tokio::signal::unix::{self, SignalKind};
use tracing::{info, warn};

const BUILD_INFO: BuildInfo = make_build_info!();
const SYSLOG_IDENTIFIER: &str = "worldcoin-orbd";

#[derive(Parser, Debug)]
#[clap(version = BUILD_INFO.version, about)]
struct Args {}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let tel_flusher = orb_telemetry::TelemetryConfig::new()
        .with_journald(SYSLOG_IDENTIFIER)
        .init();

    // Build info stuff
    let _args = Args::parse();

    let result = run().await;
    tel_flusher.flush_blocking();
    result
}

async fn run() -> Result<()> {
    info!(version = BUILD_INFO.version, "orbd starting");

    // Traverse /usr/persistent and log filesizes
    // Always runs once on startup of the orbd service
    tokio::task::spawn_blocking(orb_health::file_sizes::run)
        .await?
        .map_err(|e| warn!("orb-health::file_sizes failed: {e}"))
        .ok();

    // I can see a spear happening....someday

    let mut sigterm = unix::signal(SignalKind::terminate())?;
    let mut sigint = unix::signal(SignalKind::interrupt())?;

    tokio::select! {
        _ = sigterm.recv() => warn!("received SIGTERM"),
        _ = sigint.recv()  => warn!("received SIGINT"),
    }

    Ok(())
}
