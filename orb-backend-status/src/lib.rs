pub mod backend;
pub mod collectors;
pub mod dbus;
pub mod sender;

use crate::sender::BackendSender;
use backend::status::StatusClient;
use collectors::{
    connectivity, core_signups, net_stats, token::TokenWatcher, update_progress,
};
use color_eyre::eyre::Result;
use dbus::{intf_impl::BackendStatusImpl, setup_dbus};
use orb_build_info::{make_build_info, BuildInfo};
use orb_info::{OrbId, OrbJabilId, OrbName};
use reqwest::Url;
use std::{path::PathBuf, time::Duration};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::info;

pub const BUILD_INFO: BuildInfo = make_build_info!();

#[bon::builder(finish_fn = run)]
pub async fn program(
    dbus: zbus::Connection,
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

    let token_receiver = TokenWatcher::spawn(dbus.clone(), shutdown_token.clone()).await;

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

    // Spawn collectors
    let mut tasks: Vec<JoinHandle<()>> = vec![];

    tasks.push(net_stats::spawn_reporter(
        backend_status_impl.clone(),
        net_stats_poll_interval,
        procfs,
        shutdown_token.clone(),
    ));

    let connectivity = connectivity::spawn_watcher(
        dbus.clone(),
        Duration::from_secs(2),
        shutdown_token.clone(),
    )
    .await;

    tasks.push(connectivity.task);
    let connectivity_receiver = connectivity.receiver;

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

    sender
        .run_loop(
            backend_status_impl,
            token_receiver,
            connectivity_receiver,
            shutdown_token.clone(),
        )
        .await;

    info!("Shutting down backend-status completed");

    for task in tasks {
        task.abort();
    }

    Ok(())
}
