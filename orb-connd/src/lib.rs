use color_eyre::Result;
use derive_more::Display;
use std::path::Path;
use tokio::{fs, task::JoinHandle};

pub mod connectivity_daemon;
pub mod modem_manager;
pub mod network_manager;
pub mod secure_storage;
pub mod service;
pub mod statsd;
pub mod reporters;
pub mod wpa_ctrl;

mod utils;

pub(crate) type Tasks = Vec<JoinHandle<Result<()>>>;

#[derive(Display, Debug, PartialEq, Copy, Clone)]
pub enum OrbCapabilities {
    CellularAndWifi,
    WifiOnly,
}

impl OrbCapabilities {
    pub async fn from_sysfs(sysfs: impl AsRef<Path>) -> Self {
        let sysfs = sysfs.as_ref().join("class").join("net").join("wwan0");
        if fs::metadata(&sysfs).await.is_ok() {
            OrbCapabilities::CellularAndWifi
        } else {
            OrbCapabilities::WifiOnly
        }
    }
}
