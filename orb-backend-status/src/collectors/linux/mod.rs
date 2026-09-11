pub mod connectivity;
pub mod core_signups;
pub mod front_als;
pub mod hardware_states;
pub mod net_stats;
pub(crate) mod reroute;
pub mod token;
pub mod update_progress;

use crate::backend::types::OrbStatusApiV2;
use crate::{
    dbus::{intf_impl::BackendStatusImpl, setup_dbus},
    orb_event_stream::OrbEventStream,
};
use color_eyre::Result;
use connectivity::GlobalConnectivity;
use hardware_states::HardwareState;
use orb_messages::main::AmbientLight;
use reroute::OesReroute;
use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Duration};
use token::TokenWatcher;
use tokio::{sync::watch, task::JoinHandle};
use tokio_util::sync::CancellationToken;
use zenorb::Zenorb;

#[derive(Clone)]
pub(crate) struct ZenorbCtx {
    pub backend_status: BackendStatusImpl,
    pub connectivity_tx: watch::Sender<GlobalConnectivity>,
    pub hardware_states: Arc<tokio::sync::Mutex<HashMap<String, HardwareState>>>,
    pub front_als: Arc<tokio::sync::Mutex<Option<AmbientLight>>>,
    pub oes: OrbEventStream,
}

pub struct Collectors {
    state: BackendStatusImpl,
    connectivity_tx: watch::Sender<GlobalConnectivity>,
}

impl Collectors {
    pub async fn new(
        dbus: &zbus::Connection,
        shutdown_token: CancellationToken,
    ) -> Result<(
        Self,
        watch::Receiver<String>,
        watch::Receiver<GlobalConnectivity>,
    )> {
        let state = BackendStatusImpl::new();
        setup_dbus(dbus, state.clone()).await?;

        let token_receiver = TokenWatcher::spawn(dbus.clone(), shutdown_token).await;

        let (connectivity_tx, connectivity_receiver) =
            watch::channel(GlobalConnectivity::NotConnected);

        Ok((
            Self {
                state,
                connectivity_tx,
            },
            token_receiver,
            connectivity_receiver,
        ))
    }

    pub(crate) fn spawn_reporters(
        &self,
        dbus: zbus::Connection,
        net_stats_poll_interval: Duration,
        procfs: PathBuf,
        shutdown_token: CancellationToken,
    ) -> Vec<JoinHandle<()>> {
        vec![
            net_stats::spawn_reporter(
                self.state.clone(),
                net_stats_poll_interval,
                procfs,
                shutdown_token.clone(),
            ),
            update_progress::spawn_reporter(
                dbus.clone(),
                self.state.clone(),
                shutdown_token.clone(),
            ),
            core_signups::spawn_reporter(dbus, self.state.clone(), shutdown_token),
        ]
    }

    pub(crate) async fn subscribe(
        &self,
        zsession: &Zenorb,
        oes: OrbEventStream,
    ) -> Result<Vec<JoinHandle<()>>> {
        let ctx = ZenorbCtx {
            backend_status: self.state.clone(),
            connectivity_tx: self.connectivity_tx.clone(),
            hardware_states: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            front_als: Arc::new(tokio::sync::Mutex::new(None)),
            oes,
        };

        zsession
            .receiver(ctx)
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
            .oes_reroute(
                "core/config",
                Duration::from_millis(100),
                oes::Mode::CacheOnly,
            )
            .run()
            .await
    }

    pub(crate) async fn snapshot(&self) -> OrbStatusApiV2 {
        self.state.snapshot().to_orb_status_api_v2_req().await
    }

    pub(crate) async fn wait_for_urgent_send(&self) {
        self.state.wait_for_urgent_send().await;
    }

    pub(crate) fn clear_send_immediately(&self) {
        self.state.clear_send_immediately();
    }
}
