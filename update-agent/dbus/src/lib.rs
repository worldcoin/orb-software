//! Query the current update status:
//! ```bash
//! gdbus call --session -d org.worldcoin.UpdateProgress1 -o '/org/worldcoin/UpdateProgress1' -m \
//! org.freedesktop.DBus.Properties.Get org.worldcoin.UpdateProgress1 Status
//! ```
//!
//! Monitor for signals:
//! ```bash
//! dbus-monitor type='signal',sender='org.worldcoin.UpdateProgress1'
//! ```

use serde::{Deserialize, Serialize};
use zbus::interface;
use zbus::zvariant::{OwnedValue, Type, Value};

/// A trait representing update progress behavior.
///
/// This trait is implemented by types that can provide information about the current update status.
pub trait UpdateProgressT: Send + Sync + 'static {
    fn status(&self) -> zbus::fdo::Result<Vec<ComponentStatus>>;
}

/// A wrapper struct for types implementing [`UpdateProgressT`].
pub struct UpdateProgress<T: UpdateProgressT>(pub T);

#[derive(
    Debug, Serialize, Deserialize, Type, Clone, Copy, Eq, PartialEq, Value, OwnedValue,
)]
pub enum ComponentState {
    None = 1,
    Fetched = 2,
    Processed = 3,
    Installed = 4,
}

#[derive(
    Debug, Serialize, Deserialize, Type, Eq, PartialEq, Clone, Value, OwnedValue,
)]
pub struct ComponentStatus {
    pub name: String,
    pub state: ComponentState,
}

/// DBus interface implementation for [`UpdateProgress`].
#[interface(
    name = "org.worldcoin.UpdateProgress1",
    proxy(
        default_service = "org.worldcoin.UpdateProgress1",
        default_path = "/org/worldcoin/UpdateProgress1",
    )
)]
impl<T: UpdateProgressT> UpdateProgressT for UpdateProgress<T> {
    #[zbus(property)]
    fn status(&self) -> zbus::fdo::Result<Vec<ComponentStatus>> {
        self.0.status()
    }
}
