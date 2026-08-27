#[cfg(feature = "dbus")]
pub use crate::dbus::dbus_status::DbusStatus;
#[cfg(feature = "zenoh")]
pub use crate::dbus::zenoh_status::ZenohStatus;

use std::sync::{Arc, Mutex};
use tokio::sync::Notify;

#[derive(Clone)]
pub struct BackendStatusImpl {
    pub(crate) current_status: Arc<Mutex<CurrentStatus>>,
    /// Notify to wake up the sender loop immediately for urgent sends.
    notify_urgent: Arc<Notify>,
    /// Flag that persists until we actually send. Set by urgent events,
    /// cleared only after successful send.
    send_immediately: Arc<Mutex<bool>>,
}

#[derive(Debug, Default, Clone)]
pub struct CurrentStatus {
    /// THIS IS DEPRECATED, PLEASE DO NOT ADD ANY NEW FIELDS OR USE THIS
    /// ANYMORE. If you need to send new data types to the backend, use
    /// the OES.
    #[cfg(feature = "dbus")]
    pub dbus: DbusStatus,
    /// Status collected from zenoh (hardware states, front ALS).
    #[cfg(feature = "zenoh")]
    pub zenoh: ZenohStatus,
}

impl Default for BackendStatusImpl {
    fn default() -> Self {
        Self::new()
    }
}

impl BackendStatusImpl {
    pub fn new() -> Self {
        Self {
            current_status: Arc::new(Mutex::new(CurrentStatus::default())),
            notify_urgent: Arc::new(Notify::new()),
            send_immediately: Arc::new(Mutex::new(false)),
        }
    }

    /// Set the urgent send flag and wake up the sender loop.
    /// The flag remains set until `clear_send_immediately()` is called
    /// after a successful send.
    pub fn set_send_immediately(&self) {
        if let Ok(mut send_immediately) = self.send_immediately.lock() {
            *send_immediately = true;
        }
        self.notify_urgent.notify_one();
    }

    /// Wait for an urgent send request. Returns immediately if an urgent
    /// event has already been signaled.
    pub async fn wait_for_urgent_send(&self) {
        self.notify_urgent.notified().await;
    }

    pub fn snapshot(&self) -> CurrentStatus {
        self.current_status
            .lock()
            .map(|s| s.clone())
            .unwrap_or_default()
    }

    pub fn clear_send_immediately(&self) {
        if let Ok(mut send_immediately) = self.send_immediately.lock() {
            *send_immediately = false;
        }
    }
}
