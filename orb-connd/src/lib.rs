use color_eyre::eyre::{Result, WrapErr};
use derive_more::Display;
use orb_info::orb_os_release::{OrbOsRelease, OrbRelease};
use rusty_network_manager::NetworkManagerProxy;
use tokio::{
    fs,
    signal::unix::{self, SignalKind},
    task::JoinHandle,
};
use tracing::{info, warn};
use zbus::Connection;

mod cellular;
mod modem_manager;
mod telemetry;
mod utils;

pub(crate) type Tasks = Vec<JoinHandle<Result<()>>>;

#[derive(Display)]
pub(crate) enum OrbCapabilities {
    CellularAndWifi,
    WifiOnly,
}

impl OrbCapabilities {
    pub async fn from_fs() -> Result<Self> {
        let has_wwan_iface = fs::metadata("/sys/class/net/wwan0")
            .await
            .map(|_| true)
            .inspect_err(|e| warn!("wwan0 does not seem to exist: {e}"))
            .wrap_err("/sys/class/net/wwan0 does not exist")?;

        let cap = if has_wwan_iface {
            OrbCapabilities::CellularAndWifi
        } else {
            OrbCapabilities::WifiOnly
        };

        Ok(cap)
    }
}

// determine if cellular enabled orb (pearl or no wwan0)
// create session dbus connection
// create network manager proxy
// cellular:
//      ensure cellular is up (if on service image, make sure its disabled on startup)
//      task: start cellular telemetry (don't run cellular telemetry if not cellular capable)
// wifi:
//      wifi qr code logic (only acceptable if we have no internet)
//      netconfig qr code logic (only apply full settings if cellular enabled orb)
// service:
//      methods:
//          status (summary of current state, used for testing)
//          connect
//          wifi_qr_code
//          netconfig_qr_code
//          reset_to_default
//
//      signals:
//
pub async fn run(os_release: OrbOsRelease) -> Result<()> {
    let cap = OrbCapabilities::from_fs().await?;
    info!(
        "connd starting on Orb {} {} with capabilities: {}",
        os_release.orb_os_platform_type, os_release.release_type, cap
    );

    let dbus_conn = Connection::session().await?;
    let nm = NetworkManagerProxy::new(&dbus_conn).await?;
    let c = nm.clone();

    let mut tasks = vec![];

    tasks.extend(telemetry::start(cap).await?);

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
