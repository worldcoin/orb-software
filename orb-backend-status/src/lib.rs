pub mod args;
pub mod backend;
pub mod collectors;
pub mod dbus;

use args::Args;
use backend::status::StatusClient;
use collectors::net_stats::poll_net_stats;
use collectors::update_progress::UpdateProgressWatcher;
use color_eyre::eyre::Result;
use dbus::{intf_impl::BackendStatusImpl, setup_dbus};
use orb_backend_status_dbus::BackendStatusT;
use orb_build_info::{make_build_info, BuildInfo};
use orb_info::{OrbId, OrbJabilId, OrbName, TokenTaskHandle};
use orb_telemetry::TraceCtx;
use std::{str::FromStr, sync::Arc, time::Duration};
use tokio::{sync::watch, time::Instant};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info};
use zbus::Connection;

use crate::collectors::core_signups::CoreSignupWatcher;

pub const BUILD_INFO: BuildInfo = make_build_info!();

pub async fn run(args: &Args) -> Result<()> {
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
    let update_progress_watcher = UpdateProgressWatcher::init(&connection).await?;
    let core_signups_watcher = CoreSignupWatcher::init(&connection).await?;

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
                        if let Err(e) = backend_status_impl
                            .provide_net_stats(net_stats, TraceCtx::collect())
                        {
                            error!("failed to update net stats: {e:?}");
                        };
                    }
                    Err(e) => {
                        error!("failed to poll net stats: {e:?}");
                    }
                }
                debug!("Getting update progress from signal-based watcher");
                match update_progress_watcher.poll_update_progress().await {
                    Ok(components) => {
                        if let Err(e) = backend_status_impl
                            .provide_update_progress(components, TraceCtx::collect())
                        {
                            error!("failed to update update progress: {e:?}");
                        };
                    }
                    Err(e) => {
                        debug!("failed to get update progress: {e:?}");
                    }
                }
                debug!("Getting core signups from signal-based watcher");
                match core_signups_watcher.get_signup_state().await {
                    Ok(signup_state) => {
                        if let Err(e) = backend_status_impl
                            .provide_signup_state(signup_state, TraceCtx::collect())
                        {
                            error!("failed to update signup state: {e:?}");
                        };
                    }
                    Err(e) => {
                        error!("failed to get signup state: {e:?}");
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


