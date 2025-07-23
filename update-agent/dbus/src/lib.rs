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

/// Constants for D-Bus services, interfaces, paths, and properties
pub mod constants {
    /// D-Bus service names
    pub mod services {
        pub const UPDATE_AGENT_MANAGER: &str = "org.worldcoin.UpdateAgentManager1";
        pub const ORB_SUPERVISOR: &str = "org.worldcoin.OrbSupervisor1";
        pub const AUTH_TOKEN_MANAGER: &str = "org.worldcoin.AuthTokenManager1";
        pub const ORB_CORE: &str = "org.worldcoin.OrbCore1";
        pub const ORB_UI_STATE: &str = "org.worldcoin.OrbUiState1";
    }

    /// D-Bus object paths
    pub mod paths {
        pub const UPDATE_AGENT_MANAGER: &str = "/org/worldcoin/UpdateAgentManager1";
        pub const ORB_SUPERVISOR_MANAGER: &str = "/org/worldcoin/OrbSupervisor1/Manager";
        pub const AUTH_TOKEN_MANAGER: &str = "/org/worldcoin/AuthTokenManager1";
        pub const ORB_CORE_SIGNUP: &str = "/org/worldcoin/OrbCore1/Signup";
        pub const ORB_UI_STATE: &str = "/org/worldcoin/OrbUiState1";
    }

    /// D-Bus interface names (typically match service names)
    pub mod interfaces {
        pub const UPDATE_AGENT_MANAGER: &str = "org.worldcoin.UpdateAgentManager1";
        pub const ORB_SUPERVISOR_MANAGER: &str = "org.worldcoin.OrbSupervisor1.Manager";
        pub const AUTH_TOKEN_MANAGER: &str = "org.worldcoin.AuthTokenManager1";
        pub const ORB_CORE_SIGNUP: &str = "org.worldcoin.OrbCore1.Signup";
        pub const ORB_UI_STATE: &str = "org.worldcoin.OrbUiState1";

        /// Standard D-Bus interfaces
        pub const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
    }

    /// D-Bus property names
    pub mod properties {
        pub const PROGRESS: &str = "Progress";
        pub const OVERALL_STATUS: &str = "OverallStatus";
        pub const OVERALL_PROGRESS: &str = "OverallProgress";
    }

    /// D-Bus method and signal names
    pub mod methods {
        pub const PROPERTIES_CHANGED: &str = "PropertiesChanged";
        pub const GET: &str = "Get";
    }
}

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
    Installing = 5,
    Installed = 6,
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
    Installing = 5,
    Installed = 6,
    Rebooting = 7,
    NoNewVersion = 8,
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

/// Common update-related utilities that can be shared across orb components
pub mod common_utils {
    use crate::{ComponentState, UpdateAgentState};

    /// Maps UpdateAgentState values to their numeric representation
    pub struct UpdateAgentStateMapper;

    impl UpdateAgentStateMapper {
        pub fn from_u32(value: u32) -> Option<UpdateAgentState> {
            match value {
                1 => Some(UpdateAgentState::None),
                2 => Some(UpdateAgentState::Downloading),
                3 => Some(UpdateAgentState::Fetched),
                4 => Some(UpdateAgentState::Processed),
                5 => Some(UpdateAgentState::Installing),
                6 => Some(UpdateAgentState::Installed),
                7 => Some(UpdateAgentState::Rebooting),
                8 => Some(UpdateAgentState::NoNewVersion),
                _ => None,
            }
        }

        pub fn to_u32(state: UpdateAgentState) -> u32 {
            match state {
                UpdateAgentState::None => 1,
                UpdateAgentState::Downloading => 2,
                UpdateAgentState::Fetched => 3,
                UpdateAgentState::Processed => 4,
                UpdateAgentState::Installing => 5,
                UpdateAgentState::Installed => 6,
                UpdateAgentState::Rebooting => 7,
                UpdateAgentState::NoNewVersion => 8,
            }
        }
    }

    /// Maps ComponentState values  
    pub struct ComponentStateMapper;

    impl ComponentStateMapper {
        pub fn from_update_agent_state(state: UpdateAgentState) -> ComponentState {
            match state {
                UpdateAgentState::None => ComponentState::None,
                UpdateAgentState::Downloading => ComponentState::Downloading,
                UpdateAgentState::Fetched => ComponentState::Fetched,
                UpdateAgentState::Processed => ComponentState::Processed,
                UpdateAgentState::Installing => ComponentState::Installing,
                UpdateAgentState::Installed => ComponentState::Installed,
                UpdateAgentState::Rebooting => ComponentState::Installed, // Map rebooting to installed
                UpdateAgentState::NoNewVersion => ComponentState::None,
            }
        }
    }
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
