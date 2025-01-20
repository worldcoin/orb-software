//! Query the current update status:
//! ```bash
//! gdbus call --session -d org.worldcoin.UpdateAgentManager1 -o \
//! '/org/worldcoin/UpdateAgentManager1' -m \
//! org.freedesktop.DBus.Properties.Get org.worldcoin.UpdateAgentManager1 Progress
//! ```
//!
//! Monitor for signals:
//! ```bash
//! dbus-monitor type='signal',sender='org.worldcoin.UpdateAgentManager1'
//! ```

use serde::{Deserialize, Serialize};
use zbus::interface;
use zbus::zvariant::{OwnedValue, Type, Value};

/// A trait representing update progress behavior.
///
/// This trait is implemented by types that can provide information about the current update status.
pub trait UpdateAgentManagerT: Send + Sync + 'static {
    fn progress(&self) -> Vec<ComponentStatus>;
}

/// A wrapper struct for types implementing [`UpdateAgentManagerT`].
pub struct UpdateAgentManager<T>(pub T);

#[derive(
    Debug, Serialize, Deserialize, Type, Clone, Copy, Eq, PartialEq, Value, OwnedValue,
)]
pub enum ComponentState {
    None = 1,
    Downloading = 2,
    Fetched = 3,
    Processed = 4,
    Installed = 5,
}

#[derive(
    Debug, Serialize, Deserialize, Type, Eq, PartialEq, Clone, Value, OwnedValue,
)]
pub struct ComponentStatus {
    pub name: String,
    pub state: ComponentState,
    pub progress: u8,
}

/// DBus interface implementation for [`UpdateProgress`].
#[interface(
    name = "org.worldcoin.UpdateAgentManager1",
    proxy(
        default_service = "org.worldcoin.UpdateAgentManager1",
        default_path = "/org/worldcoin/UpdateAgentManager1",
    )
)]
impl<T: UpdateAgentManagerT> UpdateAgentManagerT for UpdateAgentManager<T> {
    #[zbus(property)]
    fn progress(&self) -> Vec<ComponentStatus> {
        self.0.progress()
    }
}
