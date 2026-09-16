use std::path::Path;
use tokio::fs;

pub mod conn_http_check;
pub mod connectivity_daemon;
pub mod mcu_util;
pub mod modem;
pub mod modem_manager;
pub mod network_manager;
pub mod reporters;
pub mod resolved;
pub mod secure_storage;
pub mod service;
pub mod systemd;
pub mod wpa_ctrl;

mod ble;
mod utils;

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub struct OrbCapabilities {
    pub cellular: bool,
    pub bluetooth: bool,
}

impl OrbCapabilities {
    pub async fn from_sysfs(sysfs: impl AsRef<Path>) -> Self {
        let class = sysfs.as_ref().join("class");

        Self {
            cellular: fs::metadata(class.join("net").join("wwan0")).await.is_ok(),
            bluetooth: fs::metadata(class.join("bluetooth").join("hci0"))
                .await
                .is_ok(),
        }
    }
}
