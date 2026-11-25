#[cfg(not(any(feature = "linux-collectors", feature = "android-collectors")))]
compile_error!("enable linux-collectors or android-collectors");

#[cfg(all(feature = "android-collectors", not(feature = "linux-collectors")))]
pub mod android;
#[cfg(all(feature = "android-collectors", not(feature = "linux-collectors")))]
pub use android::{Collectors, Config};

#[cfg(feature = "linux-collectors")]
pub mod linux;

#[cfg(feature = "linux-collectors")]
pub use linux::{
    connectivity, core_signups, front_als, hardware_states, net_stats, token,
    update_progress,
};
#[cfg(feature = "linux-collectors")]
pub use linux::{Collectors, Config};

/// Requests received during a send coalesce into one wakeup for the next snapshot.
#[derive(Default)]
pub(crate) struct UrgentSend(tokio::sync::Notify);

impl UrgentSend {
    pub(crate) fn request_urgent_send(&self) {
        self.0.notify_one();
    }

    pub(crate) async fn wait(&self) {
        self.0.notified().await;
    }
}

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
