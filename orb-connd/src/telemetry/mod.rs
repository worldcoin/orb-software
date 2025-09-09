use crate::{
    modem_manager, telemetry::{modem::Modem, net_stats::NetStats}, utils::{retry_for, State}, OrbCapabilities, Tasks
};
use color_eyre::Result;
use std::time::Duration;
use tracing::{error, info};

pub mod backend_status_reporter;
pub mod connection_state;
pub mod dd_reporter;
pub mod location;
pub mod modem;
pub mod modem_monitor;
pub mod net_stats;
pub mod signal;

// currently only modem telemetry
// later will add more
pub async fn start(cap: OrbCapabilities) -> Result<Tasks> {
    info!("starting telemetry task");
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

    Ok(vec![
        modem_monitor_handle,
        backend_status_reporter_handle,
        dd_reporter_handle,
    ])
}

async fn make_modem() -> Result<State<Modem>> {
    let modem: Result<Modem> = async {
        let modem_id = modem_manager::get_modem_id().await?;
        let sim_id = modem_manager::get_sim_id(&modem_id).await?;
        let imei = modem_manager::get_imei(&modem_id).await?;
        let iccid = modem_manager::get_iccid(sim_id).await?;
        let state = modem_manager::get_connection_state(&modem_id).await?;
        modem_manager::start_signal_refresh(&modem_id).await?;
        let net_stats = NetStats::from_wwan0().await?;

        Ok(Modem::new(modem_id, iccid, imei, state, net_stats))
    }
    .await
    .inspect_err(|e| error!("make_modem: {e}"));

    Ok(State::new(modem?))
}
