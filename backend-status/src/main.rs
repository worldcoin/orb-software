mod args;
mod backend;
mod dbus;
mod update_progress;

use std::time::Duration;

use args::Args;
use clap::Parser;
use color_eyre::eyre::Result;
use dbus::setup_dbus;
use orb_build_info::{make_build_info, BuildInfo};
use tokio_util::sync::CancellationToken;
use tracing::info;
use update_progress::UpdateProgressWatcher;
use zbus::Connection;

const BUILD_INFO: BuildInfo = make_build_info!();

const SYSLOG_IDENTIFIER: &str = "worldcoin-backend-status";

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let tel_flusher = orb_telemetry::TelemetryConfig::new()
        .with_journald(SYSLOG_IDENTIFIER)
        .init();

    let args = Args::parse();
    let result = run(&args).await;
    tel_flusher.flush().await;
    result
}

async fn run(args: &Args) -> Result<()> {
    info!("Starting backend-status: {:?}", args);

    let shutdown_token = CancellationToken::new();
    let (mut backend_status_update, _dbus_conn) =
        setup_dbus(args, shutdown_token.clone()).await?;

    let connection = Connection::session().await?;
    let mut update_progress_watcher = UpdateProgressWatcher::init(connection).await?;
    loop {
        tokio::select! {
            current_status = backend_status_update.wait_for_updates(Duration::from_secs(args.status_update_interval)) => {
                if let Some(current_status) = current_status {
                    info!("Updating backend-status");
                    backend_status_update.send_current_status(&current_status).await;
                }
            }
            Ok(components) = update_progress_watcher.wait_for_updates() => {
                info!("Updating progress");
                backend_status_update.provide_update_progress(components);
            }
            _ = shutdown_token.cancelled() => {
                info!("Shutting down backend-status initiated");
                break;
            }
        }
    }

    info!("Shutting down backend-status completed");
    Ok(())
}
