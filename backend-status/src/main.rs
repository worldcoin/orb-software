mod args;
mod backend;
mod dbus;
mod update_progress;

use args::Args;
use backend::status::StatusClient;
use clap::Parser;
use color_eyre::eyre::Result;
use dbus::{setup_dbus, BackendStatusImpl};
use orb_backend_status_dbus::BackendStatusProxy;
use orb_build_info::{make_build_info, BuildInfo};
use orb_telemetry::TraceCtx;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};
use update_progress::UpdateProgressWatcher;
use zbus::Connection;

const BUILD_INFO: BuildInfo = make_build_info!();
const SYSLOG_IDENTIFIER: &str = "worldcoin-backend-status";

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let telemetry = orb_telemetry::TelemetryConfig::new()
        .with_opentelemetry(orb_telemetry::OpentelemetryConfig::new(
            orb_telemetry::OpentelemetryAttributes {
                service_name: SYSLOG_IDENTIFIER.to_string(),
                service_version: BUILD_INFO.version.to_string(),
                additional_otel_attributes: Default::default(),
            },
        )?)
        .with_journald(SYSLOG_IDENTIFIER)
        .init();

    let args = Args::parse();
    let result = run(&args).await;

    telemetry.flush().await;
    result
}

async fn run(args: &Args) -> Result<()> {
    info!("Starting backend-status: {:?}", args);

    let shutdown_token = CancellationToken::new();
    let mut backend_status_impl = BackendStatusImpl::new(
        StatusClient::new(args, shutdown_token.clone()).await?,
        Duration::from_secs(args.status_update_interval),
        shutdown_token.clone(),
    )
    .await;

    // Setup the server and client DBus connections
    let _server_conn = setup_dbus(backend_status_impl.clone()).await?;
    let connection = Connection::session().await?;
    let backend_status_proxy = BackendStatusProxy::new(&connection).await?;
    let mut update_progress_proxy = UpdateProgressWatcher::init(&connection).await?;

    loop {
        tokio::select! {
            current_status = backend_status_impl.wait_for_updates() => {
                if let Some(current_status) = current_status {
                    info!("Updating backend-status");
                    backend_status_impl.send_current_status(&current_status).await;
                }
            }
            Ok(components) = update_progress_proxy.wait_for_updates() => {
                info!("Updating progress");
                match backend_status_proxy.provide_update_progress(components, TraceCtx::extract()).await {
                    Ok(_) => (),
                    Err(e) => {
                        error!("failed to send update progress: {e:?}");
                    }
                }
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
