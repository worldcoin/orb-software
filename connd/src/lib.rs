use color_eyre::eyre::Result;
use modem::{modem_manager, net_stats::NetStats, Modem};
use orb_info::orb_os_release::{OrbOsPlatform, OrbOsRelease};
use std::time::Duration;
use tokio::signal::unix::{self, SignalKind};
use tracing::{error, info, warn};
use utils::{retry_for, State};

mod backend_status_reporter;
mod dd_reporter;
mod modem;
mod modem_monitor;
mod utils;

pub async fn run() -> Result<()> {
    // TODO: this is temporary while this daemon only supports cellular metrics
    // Once there is more logic added relating to WiFi and Bluetooth we should remove this check
    if let OrbOsPlatform::Pearl = OrbOsRelease::read().await?.orb_os_platform_type {
        warn!("LTE is not supported on Pearl. Exiting");
        return Ok(());
    }

    info!("getting initial modem information");
    let modem = retry_for(
        Duration::from_secs(120),
        Duration::from_secs(10),
        make_modem,
    )
    .await?;

    let modem_monitor_handle =
        modem_monitor::start(modem.clone(), Duration::from_secs(20));
    let backend_status_reporter_handle =
        backend_status_reporter::start(modem.clone(), Duration::from_secs(30));
    let dd_reporter_handle = dd_reporter::start(modem, Duration::from_secs(20));

    let mut sigterm = unix::signal(SignalKind::terminate())?;
    let mut sigint = unix::signal(SignalKind::interrupt())?;

    tokio::select! {
        _ = sigterm.recv() => warn!("received SIGTERM"),
        _ = sigint.recv()  => warn!("received SIGINT"),
    }

    info!("aborting tasks and exiting gracefully");
    modem_monitor_handle.abort();
    backend_status_reporter_handle.abort();
    dd_reporter_handle.abort();

    Ok(())
}

async fn make_modem() -> Result<State<Modem>> {
    let modem: Result<Modem> = async {
        let modem_id = modem_manager::get_modem_id().await?;
        let imei = modem_manager::get_imei(&modem_id).await?;
        let iccid = modem_manager::get_iccid().await?;
        let state = modem_manager::get_connection_state(&modem_id).await?;
        modem_manager::start_signal_refresh(&modem_id).await?;
        let net_stats = NetStats::from_wwan0().await?;

        Ok(Modem::new(modem_id, iccid, imei, state, net_stats))
    }
    .await
    .inspect_err(|e| error!("make_modem: {e}"));

    Ok(State::new(modem?))
}
