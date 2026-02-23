pub mod backend;
pub mod collectors;
pub mod dbus;
pub mod sender;

use crate::sender::BackendSender;
use backend::status::StatusClient;
use collectors::{
    connectivity::{self, GlobalConnectivity},
    core_signups, front_als, hardware_states, net_stats,
    token::TokenWatcher,
    update_progress, ZenorbCtx,
};
use color_eyre::eyre::Result;
use dbus::{intf_impl::BackendStatusImpl, setup_dbus};
use orb_build_info::{make_build_info, BuildInfo};
use orb_info::{OrbId, OrbJabilId, OrbName};
use reqwest::Url;
use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Duration};
use tokio::{sync::watch, task::JoinHandle};
use tokio_util::sync::CancellationToken;
use tracing::info;
use zenorb::Zenorb as ZSession;

pub const BUILD_INFO: BuildInfo = make_build_info!();

#[bon::builder(finish_fn = run)]
pub async fn program(
    dbus: zbus::Connection,
    zsession: &ZSession,
    endpoint: Url,
    orb_os_version: String,
    orb_id: OrbId,
    orb_name: OrbName,
    orb_jabil_id: OrbJabilId,
    net_stats_poll_interval: Duration,
    sender_interval: Duration,
    sender_min_backoff: Duration,
    sender_max_backoff: Duration,
    req_timeout: Duration,
    req_min_retry_interval: Duration,
    req_max_retry_interval: Duration,
    procfs: impl Into<PathBuf>,
    shutdown_token: CancellationToken,
) -> Result<()> {
    info!("Starting backend-status, endpoint: {endpoint}, orb_id: {orb_id}, orb_name: {orb_name}, orb_jabil_id: {orb_jabil_id}");

    let backend_status_impl = BackendStatusImpl::new();

    setup_dbus(&dbus, backend_status_impl.clone()).await?;

    let token_receiver =
        TokenWatcher::spawn(dbus.clone(), shutdown_token.clone()).await;

    let status_client = StatusClient::new(
        endpoint,
        orb_os_version,
        orb_id,
        orb_name,
        orb_jabil_id,
        req_timeout,
        req_min_retry_interval,
        req_max_retry_interval,
    )
    .await?;

    let sender = BackendSender::new(
        status_client,
        sender_interval,
        sender_min_backoff,
        sender_max_backoff,
    );

    // Spawn non-zenorb collectors
    let mut tasks: Vec<JoinHandle<()>> = vec![];

    tasks.push(net_stats::spawn_reporter(
        backend_status_impl.clone(),
        net_stats_poll_interval,
        procfs,
        shutdown_token.clone(),
    ));

    tasks.push(update_progress::spawn_reporter(
        dbus.clone(),
        backend_status_impl.clone(),
        shutdown_token.clone(),
    ));

    tasks.push(core_signups::spawn_reporter(
        dbus.clone(),
        backend_status_impl.clone(),
        shutdown_token.clone(),
    ));

    // Build unified zenorb context and single receiver
    let (connectivity_tx, connectivity_receiver) =
        watch::channel(GlobalConnectivity::NotConnected);

    let zenorb_ctx = ZenorbCtx {
        backend_status: backend_status_impl.clone(),
        connectivity_tx,
        hardware_states: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        front_als: Arc::new(tokio::sync::Mutex::new(None)),
    };

    let receiver = zsession.receiver(zenorb_ctx);
    let receiver = connectivity::register(receiver);
    let receiver = hardware_states::register(receiver);
    let receiver = front_als::register(receiver);

    let zenorb_tasks = receiver.run().await?;

    // Spawn a single shutdown task for all zenorb subscribers
    let shutdown = shutdown_token.clone();
    tasks.push(tokio::spawn(async move {
        shutdown.cancelled().await;
        for task in zenorb_tasks {
            task.abort();
        }
    }));

    sender
        .run_loop(
            backend_status_impl,
            token_receiver,
            connectivity_receiver,
            shutdown_token.clone(),
        )
        .await;

    for task in tasks {
        task.abort();
    }

    Ok(())
}
