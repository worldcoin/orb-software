use color_eyre::eyre::{Result, WrapErr};
use derive_more::Display;
use modem_manager::ModemManager;
use orb_info::orb_os_release::OrbOsRelease;
use service::ConndService;
use statsd::StatsdClient;
use std::path::Path;
use tokio::{
    fs,
    signal::unix::{self, SignalKind},
    task::JoinHandle,
};
use tracing::{info, warn};

mod cellular;
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
//      - [ ] cfg and establish on startup (worker)
//
//  wifi:
//      - [ ] import old config on startup
//
// conn general:
//      - [ ] apply saved config on startup
//      - [ ] wait 5 seconds, if still no internet ask for wifi qr code
//      - [ ] smart switching new url
//
//  dbus (service):
//      - [x] add wifi profile
//      - [x] remove wifi profile
//      - [ ] apply netconfig qr
//      - [ ] apply wifi qr (with restrictions)
//            only available if there is no internet
//      - [ ] apply magic wifi qr reset
//            deletes all credentials, accepts any wifi qr code for the next 10min
//      - [ ] toggle smart switching
//      - [x] create soft ap, args ssid pw
//            returns not impled error
// orb-core:
//  - [ ] call here via dbus
//  - [ ] disable backend status
//  - [ ] ignore attest token
//
// TODO: do NOT allow profiles to be added with same name as default cellular profile

#[bon::builder(finish_fn = run)]
pub async fn program(
    sysfs: impl AsRef<Path>,
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

    let mut tasks =
        vec![ConndService::new(system_dbus, os_release.release_type).spawn()];

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
