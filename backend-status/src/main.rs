mod args;
mod backend;
mod dbus;
mod net_stats;
mod update_progress;

use args::Args;
use backend::status::StatusClient;
use clap::Parser;
use color_eyre::eyre::Result;
use dbus::{intf_impl::BackendStatusImpl, setup_dbus};
use net_stats::poll_net_stats;
use orb_backend_status_dbus::BackendStatusProxy;
use orb_build_info::{make_build_info, BuildInfo};
use orb_info::{OrbId, OrbJabilId, OrbName, TokenTaskHandle};
use orb_telemetry::TraceCtx;
use std::{str::FromStr, sync::Arc, time::Duration};
use tokio::{sync::watch, time::Instant};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info};
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
    let orb_name = OrbName::read().await.map(Some).unwrap_or(None);
    let jabil_id = OrbJabilId::read().await.map(Some).unwrap_or(None);
    info!(
        "backend-status orb_id: {} orb_name: {:?} jabil_id: {:?}",
        orb_id, orb_name, jabil_id
    );

    // setup backend status handler
    let mut backend_status_impl = BackendStatusImpl::new(
        StatusClient::new(args, orb_id, orb_name, jabil_id, token_receiver).await?,
        Duration::from_secs(args.status_update_interval),
        shutdown_token.clone(),
    )
    .await;

    // Setup the server and client DBus connections
    let _server_conn = setup_dbus(backend_status_impl.clone()).await?;
    let connection = Connection::session().await?;
    let backend_status_proxy = BackendStatusProxy::new(&connection).await?;
    let mut update_progress_watcher = UpdateProgressWatcher::init(&connection).await?;

    // Setup the polling interval
    let polling_interval = Duration::from_secs(args.polling_interval);
    let sleep = tokio::time::sleep(polling_interval);
    tokio::pin!(sleep);

    loop {
        tokio::select! {
            () = backend_status_impl.wait_for_updates() => {
                    backend_status_impl.send_current_status().await;
            }
            () = &mut sleep => {
                debug!("Polling net stats");
                match poll_net_stats().await {
                    Ok(net_stats) => {
                        match backend_status_proxy.provide_net_stats(net_stats, TraceCtx::collect()).await {
                            Ok(_) => (),
                            Err(e) => {
                                error!("failed to send net stats: {e:?}");
                            }
                        }
                    }
                    Err(e) => {
                        error!("failed to poll net stats: {e:?}");
                    }
                }
                debug!("Getting update progress from signal-based watcher");
                match update_progress_watcher.poll_update_progress().await {
                    Ok(components) => {
                        match backend_status_proxy.provide_update_progress(components, TraceCtx::collect()).await {
                            Ok(_) => (),
                            Err(e) => {
                                error!("failed to send update progress: {e:?}");
                            }
                        }
                    }
                    Err(e) => {
                        debug!("failed to get update progress: {e:?}");
                    }
                }
                sleep.as_mut().reset(Instant::now() + polling_interval);
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
