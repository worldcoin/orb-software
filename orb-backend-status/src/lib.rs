pub mod args;
pub mod backend;
pub mod collectors;
pub mod dbus;
pub mod sender;

use args::Args;
use backend::status::StatusClient;
use collectors::net_stats::poll_net_stats;
use collectors::update_progress::UpdateProgressWatcher;
use collectors::token::TokenWatcher;
use color_eyre::eyre::Result;
use dbus::{intf_impl::BackendStatusImpl, setup_dbus};
use orb_backend_status_dbus::BackendStatusT;
use orb_build_info::{make_build_info, BuildInfo};
use orb_info::{OrbId, OrbJabilId, OrbName};
use orb_telemetry::TraceCtx;
use std::{str::FromStr, sync::Arc, time::Duration};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info};
use zbus::Connection;

use crate::collectors::core_signups::CoreSignupWatcher;
use crate::sender::BackendSender;

pub const BUILD_INFO: BuildInfo = make_build_info!();

pub async fn run(args: &Args) -> Result<()> {
    info!("Starting backend-status: {:?}", args);

    let shutdown_token = CancellationToken::new();
    let args = Arc::new(args.clone());

    // Setup backend-status handler & DBus server as early as possible, to avoid cascading
    // effects on dependents.
    let backend_status_impl = BackendStatusImpl::new(shutdown_token.clone());

    let _server_conn = setup_dbus(backend_status_impl.clone()).await?;
    let connection = Connection::session().await?;

    // Token comes either from args (static) or via a dedicated watcher (dynamic).
    let token_receiver = if let Some(token) = args.orb_token.clone() {
        let (token_sender, token_receiver) = tokio::sync::watch::channel(String::new());
        let _ = token_sender.send(token);
        token_receiver
    } else {
        TokenWatcher::spawn(connection.clone(), shutdown_token.clone())
    };

    // One-time startup identity read. If this fails, we keep running (DBus stays up),
    // but we won't be able to upload to the backend until it is fixed.
    let orb_id = if let Some(id) = args.orb_id.clone() {
        match OrbId::from_str(&id) {
            Ok(id) => Some(id),
            Err(e) => {
                error!("failed to parse orb id (will not retry): {e:?}");
                None
            }
        }
    } else {
        match OrbId::read().await {
            Ok(id) => Some(id),
            Err(e) => {
                error!("failed to read orb id (will not retry): {e:?}");
                None
            }
        }
    };

    let orb_name = OrbName::read().await.ok();
    let jabil_id = OrbJabilId::read().await.ok();

    let sender = if let Some(orb_id) = orb_id {
        info!(
            "backend-status orb_id: {} orb_name: {:?} jabil_id: {:?}",
            orb_id, orb_name, jabil_id
        );

        match StatusClient::new(&args, orb_id, orb_name, jabil_id).await {
            Ok(client) => Some(BackendSender::new(client)),
            Err(e) => {
                error!("failed to init status sender (will not retry): {e:?}");
                None
            }
        }
    } else {
        error!("status sender disabled due to missing orb id");
        None
    };

    let update_progress_watcher = match UpdateProgressWatcher::init(&connection).await
    {
        Ok(w) => Some(w),
        Err(e) => {
            error!("failed to init update progress watcher: {e:?}");
            None
        }
    };

    let core_signups_watcher = match CoreSignupWatcher::init(&connection).await {
        Ok(w) => Some(w),
        Err(e) => {
            error!("failed to init core signups watcher: {e:?}");
            None
        }
    };

    // Setup the polling interval
    let polling_interval = Duration::from_secs(args.polling_interval);
    let sleep = tokio::time::sleep(polling_interval);
    tokio::pin!(sleep);

    // Sender loop parameters (manager diagram style)
    let mut changed_backoff = Duration::from_secs(1);
    let max_backoff = Duration::from_secs(30);
    let mut last_send = Instant::now() - Duration::from_secs(args.status_update_interval);

    loop {
        tokio::select! {
            () = backend_status_impl.wait_for_change_or_shutdown() => {
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
                if let Some(update_progress_watcher) = update_progress_watcher.as_ref() {
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
                }
                debug!("Getting core signups from signal-based watcher");
                if let Some(core_signups_watcher) = core_signups_watcher.as_ref() {
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
                }
                sleep.as_mut().reset(Instant::now() + polling_interval);
            }
            _ = shutdown_token.cancelled() => {
                info!("Shutting down backend-status initiated");
                break;
            }
        }

        // Try send after any wake-up (change or polling tick).
        let Some(sender) = sender.as_ref() else {
            continue;
        };

        let interval_elapsed =
            last_send.elapsed() >= Duration::from_secs(args.status_update_interval);
        let should_attempt = backend_status_impl.changed()
            && (backend_status_impl.should_send_immediately() || interval_elapsed);

        if !should_attempt {
            continue;
        }

        let token = token_receiver.borrow().clone();
        let snapshot = backend_status_impl.snapshot();

        match sender.send_snapshot(&snapshot, &token).await {
            Ok(_) => {
                backend_status_impl.clear_changed();
                backend_status_impl.clear_send_immediately();
                last_send = Instant::now();
                changed_backoff = Duration::from_secs(1);
            }
            Err(e) => {
                error!("failed to send status (will backoff): {e:?}");
                tokio::time::sleep(changed_backoff).await;
                changed_backoff = (changed_backoff * 2).min(max_backoff);
            }
        }
    }

    info!("Shutting down backend-status completed");

    Ok(())
}


