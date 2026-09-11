pub mod linux;

pub use linux::Collectors;
pub use linux::{
    connectivity, core_signups, front_als, hardware_states, net_stats, token,
    update_progress,
};

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
