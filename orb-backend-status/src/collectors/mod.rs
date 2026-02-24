pub mod connectivity;
pub mod core_signups;
pub mod front_als;
pub mod hardware_states;
pub mod net_stats;
pub mod token;
pub mod update_progress;

use crate::dbus::intf_impl::BackendStatusImpl;
use connectivity::GlobalConnectivity;
use hardware_states::HardwareState;
use orb_messages::main::AmbientLight;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{watch, Mutex};

#[derive(Clone)]
pub(crate) struct ZenorbCtx {
    pub backend_status: BackendStatusImpl,
    pub connectivity_tx: watch::Sender<GlobalConnectivity>,
    pub hardware_states: Arc<Mutex<HashMap<String, HardwareState>>>,
    pub front_als: Arc<Mutex<Option<AmbientLight>>>,
}
