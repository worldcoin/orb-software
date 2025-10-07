use color_eyre::eyre::Result;
use derive_more::Display;
use modem_manager::ModemManager;
use orb_info::orb_os_release::OrbOsRelease;
use service::ConndService;
use statsd::StatsdClient;
use std::path::Path;
use tokio::{
    fs::{self},
    task::JoinHandle,
};
use tracing::{info, warn};

pub mod modem_manager;
pub mod network_manager;
pub mod service;
pub mod statsd;
pub mod telemetry;
mod utils;

#[bon::builder(finish_fn = run)]
pub async fn program(
    sysfs: impl AsRef<Path>,
    wpa_conf_dir: impl AsRef<Path>,
    system_bus: zbus::Connection,
    session_bus: zbus::Connection,
    os_release: OrbOsRelease,
    statsd_client: impl StatsdClient,
    modem_manager: impl ModemManager,
) -> Result<Tasks> {
    let sysfs = sysfs.as_ref().to_path_buf();
    let cap = OrbCapabilities::from_sysfs(&sysfs).await?;
    info!(
        "connd starting on Orb {} {} with capabilities: {}",
        os_release.orb_os_platform_type, os_release.release_type, cap
    );

    let connd = ConndService::new(
        session_bus.clone(),
        system_bus.clone(),
        os_release.release_type,
        os_release.orb_os_platform_type,
    );

    connd.setup_default_profiles().await?;

    if let Err(e) = connd.import_wpa_conf(wpa_conf_dir).await {
        warn!("failed to import legacy wpa config {e}");
    }

    if let Err(e) = connd.ensure_networking_enabled().await {
        warn!("failed to ensure networking is enabled {e}");
    }

    let mut tasks = vec![connd.spawn()];

    tasks.extend(
        telemetry::spawn(
            system_bus,
            session_bus,
            modem_manager,
            statsd_client,
            sysfs,
            cap,
        )
        .await?,
    );

    Ok(tasks)
}

pub(crate) type Tasks = Vec<JoinHandle<Result<()>>>;

#[derive(Display)]
pub enum OrbCapabilities {
    CellularAndWifi,
    WifiOnly,
}

impl OrbCapabilities {
    pub async fn from_sysfs(sysfs: impl AsRef<Path>) -> Result<Self> {
        let sysfs = sysfs.as_ref().join("class").join("net").join("wwan0");
        let cap = if fs::metadata(&sysfs).await.is_ok() {
            OrbCapabilities::CellularAndWifi
        } else {
            OrbCapabilities::WifiOnly
        };

        Ok(cap)
    }
}
