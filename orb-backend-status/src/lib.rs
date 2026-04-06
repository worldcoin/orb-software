pub mod backend;
pub mod collectors;
pub mod dbus;
pub mod orb_event_stream;
pub mod sender;

use crate::{
    orb_event_stream::{reroute::OesReroute, OrbEventStream},
    sender::BackendSender,
};
use backend::client::StatusClient;
use collectors::{
    connectivity::{self, GlobalConnectivity},
    core_signups, front_als, hardware_states, net_stats, oes_collector,
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

    // Build unified zenorb context and single receiver
    let (connectivity_tx, connectivity_receiver) =
        watch::channel(GlobalConnectivity::NotConnected);

    let status_client = StatusClient::builder()
        .orb_id(orb_id)
        .orb_name(orb_name)
        .jabil_id(orb_jabil_id)
        .orb_os_version(orb_os_version)
        .endpoint(endpoint)
        .req_timeout(req_timeout)
        .min_req_retry_interval(req_min_retry_interval)
        .max_req_retry_interval(req_max_retry_interval)
        .attest_token_rx(token_receiver)
        .connectivity_rx(connectivity_receiver.clone())
        .build();

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

    let oes = OrbEventStream::start(status_client.clone(), shutdown_token.clone());

    let zenorb_ctx = ZenorbCtx {
        backend_status: backend_status_impl.clone(),
        connectivity_tx,
        hardware_states: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        front_als: Arc::new(tokio::sync::Mutex::new(None)),
        oes: oes.clone(),
    };

    let zenorb_tasks = zsession
        .receiver(zenorb_ctx)
        .querying_subscriber(
            "connd/oes/active_connections",
            Duration::from_millis(15),
            connectivity::handle_connection_event,
        )
        .querying_subscriber(
            hardware_states::HARDWARE_STATUS_KEY_EXPR,
            Duration::from_millis(100),
            hardware_states::handle_hardware_state_event,
        )
        .querying_subscriber(
            front_als::FRONT_ALS_KEY_EXPR,
            Duration::from_millis(100),
            front_als::handle_front_als_event,
        )
        .subscriber(orb_event_stream::KEY_EXPR, oes_collector::handler)
        .oes_reroute(
            "core/config",
            Duration::from_millis(100),
            oes::Mode::CacheOnly,
        )
        .run()
        .await?;

    let sender = BackendSender::new(status_client.clone(), oes, sender_interval);
    sender
        .run_loop(backend_status_impl, shutdown_token.clone())
        .await;

    // Spawn a single shutdown task for all zenorb subscribers
    let shutdown = shutdown_token.clone();
    tasks.push(tokio::spawn(async move {
        shutdown.cancelled().await;
        for task in zenorb_tasks {
            task.abort();
        }
    }));

    for task in tasks {
        task.abort();
    }

    Ok(())
}
