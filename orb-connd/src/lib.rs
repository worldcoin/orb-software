use color_eyre::eyre::{Result, WrapErr};
use derive_more::Display;
use modem_manager::ModemManager;
use orb_info::orb_os_release::OrbOsRelease;
use service::ConndService;
use statsd::StatsdClient;
use std::path::Path;
use tokio::{
    fs::{self},
    signal::unix::{self, SignalKind},
    task::JoinHandle,
};
use tracing::{info, warn};

pub mod modem_manager;
pub mod network_manager;
pub mod service;
pub mod statsd;
pub mod telemetry;
mod utils;

//  telemetry:
//      - [t] modem telemetry collector (worker)
//      - [t] modem statsd reporter (worker)
//      - [t] modem backend status reporter (worker)
//      - [ ] connectivity config backend status reporter (worker)
//            applied config
//            current ssids
//
//  conn cellular:
//      - [x] cfg and establish on startup (worker)
//
//  wifi:
//      - [x] import old config on startup
//      - [x] create default config on startup
//
// conn general:
//      - [x] smart switching new url
//
//  dbus (service):
//      - [x] add wifi profile
//      - [x] remove wifi profile
//      - [x] apply netconfig qr
//      - [x] apply wifi qr (with restrictions)
//            only available if there is no internet
//      - [x] apply magic wifi qr reset
//            deletes all credentials, accepts any wifi qr code for the next 10min
//      - [x] create soft ap, args ssid pw
//            returns not impled error
//
// orb-core and backend-connect:
//  - [ ] call here via dbus
//
// TODO: do NOT allow profiles to be added with same name as default cellular profile

pub const DEFAULT_CELLULAR_PROFILE: &str = "cellular";
pub const DEFAULT_CELLULAR_APN: &str = "em";
pub const DEFAULT_CELLULAR_IFACE: &str = "cdc-wdm0";
pub const DEFAULT_WIFI_SSID: &str = "hotspot";
pub const DEFAULT_WIFI_PSK: &str = "easytotypehardtoguess";
pub const MAGIC_QR_TIMESPAN_MIN: i64 = 10;

#[bon::builder(finish_fn = run)]
pub async fn program(
    sysfs: impl AsRef<Path>,
    wpa_conf_dir: impl AsRef<Path>,
    system_dbus: zbus::Connection,
    session_dbus: zbus::Connection,
    os_release: OrbOsRelease,
    statsd_client: impl StatsdClient,
    modem_manager: impl ModemManager,
) -> Result<()> {
    let sysfs = sysfs.as_ref().to_path_buf();
    let cap = OrbCapabilities::from_sysfs(&sysfs).await?;
    info!(
        "connd starting on Orb {} {} with capabilities: {}",
        os_release.orb_os_platform_type, os_release.release_type, cap
    );

    let connd = ConndService::new(
        session_dbus.clone(),
        system_dbus,
        os_release.release_type,
        os_release.orb_os_platform_type,
    );

    connd.setup_default_profiles().await?;
    if let Err(e) = connd.import_wpa_conf(wpa_conf_dir).await {
        warn!("failed to import legacy wpa config {e}");
    }

    let mut tasks = vec![connd.spawn()];

    tasks.extend(
        telemetry::spawn(session_dbus, modem_manager, statsd_client, sysfs, cap)
            .await?,
    );

    let mut sigterm = unix::signal(SignalKind::terminate())?;
    let mut sigint = unix::signal(SignalKind::interrupt())?;

    tokio::select! {
        _ = sigterm.recv() => warn!("received SIGTERM"),
        _ = sigint.recv()  => warn!("received SIGINT"),
    }

    info!("aborting tasks and exiting gracefully");

    for handle in tasks {
        handle.abort();
    }

    Ok(())
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
        let has_wwan_iface = fs::metadata(&sysfs)
            .await
            .map(|_| true)
            .inspect_err(|e| warn!("wwan0 does not seem to exist: {e}"))
            .wrap_err_with(|| format!("{sysfs:?} does not exist"))?;

        let cap = if has_wwan_iface {
            OrbCapabilities::CellularAndWifi
        } else {
            OrbCapabilities::WifiOnly
        };

        Ok(cap)
    }
}
