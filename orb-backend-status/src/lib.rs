pub mod args;
pub mod backend;
pub mod collectors;
pub mod dbus;
pub mod sender;

use args::Args;
use backend::status::StatusClient;
use collectors::{core_signups, net_stats, token::TokenWatcher, update_progress};
use color_eyre::eyre::Result;
use dbus::{intf_impl::BackendStatusImpl, setup_dbus};
use orb_build_info::{make_build_info, BuildInfo};
use orb_info::{OrbId, OrbJabilId, OrbName};
use std::{str::FromStr, sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};
use zbus::Connection;

use crate::sender::BackendSender;

pub const BUILD_INFO: BuildInfo = make_build_info!();

pub async fn run(args: &Args, shutdown_token: CancellationToken) -> Result<()> {
    info!("Starting backend-status: {:?}", args);

    let args = Arc::new(args.clone());

    // Setup backend-status handler & DBus server as early as possible, to avoid cascading
    // effects on dependents.
    let backend_status_impl = BackendStatusImpl::new();

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

    // Spawn collectors (they encapsulate their own retry/backoff for dbus subscription).
    let _net_stats = net_stats::spawn_reporter(
        backend_status_impl.clone(),
        Duration::from_secs(args.polling_interval),
        shutdown_token.clone(),
    );
    let _update_progress = update_progress::spawn_reporter(
        connection.clone(),
        backend_status_impl.clone(),
        shutdown_token.clone(),
    );
    let _signup = core_signups::spawn_reporter(
        connection.clone(),
        backend_status_impl.clone(),
        shutdown_token.clone(),
    );

    if let Some(sender) = sender {
        crate::sender::run_loop(
            backend_status_impl,
            sender,
            token_receiver,
            Duration::from_secs(args.status_update_interval),
            shutdown_token.clone(),
        )
        .await;
    } else {
        shutdown_token.cancelled().await;
    }

    info!("Shutting down backend-status completed");

    Ok(())
}


