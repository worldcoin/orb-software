pub mod connectivity;
pub mod core_signups;
pub mod front_als;
pub mod hardware_states;
pub mod net_stats;
pub(crate) mod reroute;
pub mod token;
pub mod update_progress;

use crate::backend::types::OrbStatusApiV2;
use crate::{dbus::intf_impl::BackendStatusImpl, orb_event_stream::OrbEventStream};
use connectivity::GlobalConnectivity;
use hardware_states::HardwareState;
use orb_messages::main::AmbientLight;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::watch;

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
}

impl Collectors {
    pub fn new(state: BackendStatusImpl) -> Self {
        Self { state }
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
