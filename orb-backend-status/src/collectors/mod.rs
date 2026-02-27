pub mod connectivity;
pub mod core_signups;
pub mod front_als;
pub mod hardware_states;
pub mod net_stats;
pub mod oes;
pub mod token;
pub mod update_progress;

use crate::dbus::intf_impl::BackendStatusImpl;
use connectivity::GlobalConnectivity;
use hardware_states::HardwareState;
use orb_messages::main::AmbientLight;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Instant,
};
use tokio::sync::watch;

#[derive(Clone)]
pub(crate) struct ZenorbCtx {
    pub backend_status: BackendStatusImpl,
    pub connectivity_tx: watch::Sender<GlobalConnectivity>,
    pub hardware_states: Arc<tokio::sync::Mutex<HashMap<String, HardwareState>>>,
    pub front_als: Arc<tokio::sync::Mutex<Option<AmbientLight>>>,
    pub oes_tx: flume::Sender<oes::Event>,
    pub oes_throttle: Arc<Mutex<HashMap<String, Instant>>>,
}
