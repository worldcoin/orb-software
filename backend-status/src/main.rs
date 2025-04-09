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
use orb_info::{OrbId, TokenTaskHandle};
use orb_telemetry::TraceCtx;
use std::{str::FromStr, sync::Arc, time::Duration};
use tokio::sync::watch;
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

    // Get token from args or DBus
    let mut _token_task: Option<Arc<TokenTaskHandle>> = None;
    let token_receiver = if let Some(token) = args.orb_token.clone() {
        let (_, receiver) = watch::channel(token);
        receiver
    } else {
        let connection = Connection::session().await?;
        _token_task = Some(Arc::new(
            TokenTaskHandle::spawn(&connection, &shutdown_token).await?,
        ));
        _token_task.as_ref().unwrap().token_recv.to_owned()
    };

    // Get orb id from args/env or run orb-id to get it
    let orb_id = if let Some(id) = args.orb_id.clone() {
        OrbId::from_str(&id)
            .map_err(|e| eyre::eyre!("Failed to parse orb id: {}", e))?
    } else {
        OrbId::read().await?
    };
    info!("backend-status orb_id: {}", orb_id);

    // setup backend status handler
    let mut backend_status_impl = BackendStatusImpl::new(
        StatusClient::new(args, orb_id, token_receiver).await?,
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
                    let wifi_networks = current_status.wifi_networks.is_some();
                    let update_progress = current_status.update_progress.is_some();
                    info!(?wifi_networks, ?update_progress, "Updating backend-status: wifi:{wifi_networks}, update:{update_progress}");
                    backend_status_impl.send_current_status(&current_status).await;
                }
            }
            Ok(components) = update_progress_proxy.wait_for_updates() => {
                info!("Updating progress");
                match backend_status_proxy.provide_update_progress(components, TraceCtx::collect()).await {
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
