use color_eyre::eyre::Result;
use modem::{modem_manager, Modem};
use orb_info::orb_os_release::{OrbOsPlatform, OrbOsRelease};
use std::time::Duration;
use utils::{retry_for, State};

mod backend_status_reporter;
mod dd_reporter;
mod modem;
mod modem_monitor;
mod utils;

pub async fn run() -> Result<()> {
    if let OrbOsPlatform::Pearl = OrbOsRelease::read().await?.orb_os_platform_type {
        println!("LTE is not supported on Pearl. Exiting");
        return Ok(());
    }

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
    let dd_reporter_handle =
        backend_status_reporter::start(modem, Duration::from_secs(20));

    // TODO: catch sigkill or whatever, handle termination gracefully
    tokio::select! {
        _ = modem_monitor_handle => {}
        _ = backend_status_reporter_handle => {}
        _ = dd_reporter_handle => {}
    }

    Ok(())
}

async fn make_modem() -> Result<State<Modem>> {
    let modem_id = modem_manager::get_modem_id().await?;
    let imei = modem_manager::get_imei(&modem_id).await?;
    let iccid = modem_manager::get_iccid().await?;
    let state = modem_manager::get_connection_state(&modem_id).await?;
    modem_manager::start_signal_refresh(&modem_id).await?;

    let modem = Modem::new(modem_id, iccid, imei, state);
    Ok(State::new(modem))
}
