#[cfg(feature = "zenoh")]
pub mod connectivity;
#[cfg(feature = "dbus")]
pub mod core_signups;
#[cfg(feature = "zenoh")]
pub mod front_als;
#[cfg(feature = "zenoh")]
pub mod hardware_states;
#[cfg(feature = "dbus")]
pub mod net_stats;
#[cfg(feature = "zenoh")]
pub mod oes_collector;
#[cfg(feature = "dbus")]
pub mod token;
#[cfg(feature = "dbus")]
pub mod update_progress;

/// The daemon's overall network connectivity, as last observed via zenoh.
/// Feature-independent (unlike the `connectivity` module above, which is
/// the zenoh subscriber that actually updates it): [`crate::backend::client::StatusClient`]
/// always needs a receiver for this, even when built without the `zenoh`
/// feature - it just never changes from `NotConnected` in that case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GlobalConnectivity {
    Connected { ssid: Option<String> },
    NotConnected,
}

impl GlobalConnectivity {
    pub fn is_connected(&self) -> bool {
        matches!(self, Self::Connected { .. })
    }

    pub fn ssid(&self) -> Option<&str> {
        match self {
            Self::Connected { ssid } => ssid.as_deref(),
            Self::NotConnected => None,
        }
    }
}

#[cfg(feature = "zenoh")]
use crate::{dbus::intf_impl::BackendStatusImpl, orb_event_stream::OrbEventStream};
#[cfg(feature = "zenoh")]
use hardware_states::HardwareState;
#[cfg(feature = "zenoh")]
use orb_messages::main::AmbientLight;
#[cfg(feature = "zenoh")]
use std::{collections::HashMap, sync::Arc};
#[cfg(feature = "zenoh")]
use tokio::sync::watch;

#[cfg(feature = "zenoh")]
#[derive(Clone)]
pub(crate) struct ZenorbCtx {
    pub backend_status: BackendStatusImpl,
    pub connectivity_tx: watch::Sender<GlobalConnectivity>,
    pub hardware_states: Arc<tokio::sync::Mutex<HashMap<String, HardwareState>>>,
    pub front_als: Arc<tokio::sync::Mutex<Option<AmbientLight>>>,
    pub oes: OrbEventStream,
}
