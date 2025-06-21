//! Query the current update status:
//! ```bash
//! gdbus call --session -d org.worldcoin.UpdateAgentManager1 -o \
//! '/org/worldcoin/UpdateAgentManager1' -m \
//! org.freedesktop.DBus.Properties.Get org.worldcoin.UpdateAgentManager1 Progress
//! ```
//!
//! Query the overall update status:
//! ```bash
//! gdbus call --session -d org.worldcoin.UpdateAgentManager1 -o \
//! '/org/worldcoin/UpdateAgentManager1' -m \
//! org.freedesktop.DBus.Properties.Get org.worldcoin.UpdateAgentManager1 OverallStatus
//! ```
//!
//! Query the overall update progress:
//! ```bash
//! gdbus call --session -d org.worldcoin.UpdateAgentManager1 -o \
//! '/org/worldcoin/UpdateAgentManager1' -m \
//! org.freedesktop.DBus.Properties.Get org.worldcoin.UpdateAgentManager1 OverallProgress
//! ```
//!
//! Monitor for signals:
//! ```bash
//! export DBUS_SESSION_BUS_ADDRESS=unix:path=/tmp/worldcoin_bus_socket
//! dbus-monitor --session type='signal',sender='org.worldcoin.UpdateAgentManager1'
//! ```

use serde::{Deserialize, Serialize};
use zbus::interface;
use zbus::zvariant::{OwnedValue, Type, Value};

/// A trait representing update progress behavior.
///
/// This trait is implemented by types that can provide information about the current update status.
/// It abstracts the behavior to allow multiple implementations, enabling dependency injection,
/// mocking for tests, and sharing the same interface across both client and server code.
pub trait UpdateAgentManagerT: Send + Sync + 'static {
    fn progress(&self) -> Vec<ComponentStatus>;
    fn overall_status(&self) -> UpdateAgentState;
    fn overall_progress(&self) -> u8;
}

/// A wrapper struct for types implementing [`UpdateAgentManagerT`].
pub struct UpdateAgentManager<T>(pub T);

#[derive(
    Debug,
    Serialize,
    Deserialize,
    Type,
    Clone,
    Copy,
    Eq,
    PartialEq,
    Value,
    OwnedValue,
    Default,
)]
pub enum ComponentState {
    #[default]
    None = 1,
    Downloading = 2,
    Fetched = 3,
    Processed = 4,
    Installed = 5,
}

#[derive(
    Debug,
    Serialize,
    Deserialize,
    Type,
    Clone,
    Copy,
    Eq,
    PartialEq,
    Value,
    OwnedValue,
    Default,
)]
pub enum UpdateAgentState {
    #[default]
    None = 1,
    Downloading = 2,
    Fetched = 3,
    Processed = 4,
    Installed = 5,
    Rebooting = 6,
    NoNewVersion = 7,
}

#[derive(
    Debug, Serialize, Deserialize, Type, Eq, PartialEq, Clone, Value, OwnedValue,
)]
pub struct ComponentStatus {
    /// Component Name
    pub name: String,
    /// Current state of a component
    pub state: ComponentState,
    /// Progress through the current state (0-100)
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

    #[zbus(property)]
    fn overall_status(&self) -> UpdateAgentState {
        self.0.overall_status()
    }

    #[zbus(property)]
    fn overall_progress(&self) -> u8 {
        self.0.overall_progress()
    }
}
