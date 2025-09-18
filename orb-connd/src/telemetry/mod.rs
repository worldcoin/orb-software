use crate::{
    modem_manager::{self, ModemManager},
    telemetry::{modem_status::ModemStatus, net_stats::NetStats},
    utils::{retry_for, State},
    OrbCapabilities, Tasks,
};
use color_eyre::{eyre::ContextCompat, Result};
use std::time::Duration;
use tracing::{error, info};

pub mod backend_status_reporter;
pub mod dd_reporter;
pub mod location;
pub mod modem_monitor;
pub mod modem_status;
pub mod net_stats;

// currently only modem telemetry
// later will add more
pub async fn start(mm: impl ModemManager, cap: OrbCapabilities) -> Result<Tasks> {
    info!("starting telemetry task");
    info!("getting initial modem information");
    let modem = retry_for(Duration::from_secs(120), Duration::from_secs(10), || {
        make_modem(&mm)
    })
    .await?;

    let modem_monitor_handle =
        modem_monitor::start(mm, modem.clone(), Duration::from_secs(20));
    let backend_status_reporter_handle =
        backend_status_reporter::start(modem.clone(), Duration::from_secs(30));
    let dd_reporter_handle = dd_reporter::start(modem, Duration::from_secs(20));

    Ok(vec![
        modem_monitor_handle,
        backend_status_reporter_handle,
        dd_reporter_handle,
    ])
}

async fn make_modem(mm: &impl ModemManager) -> Result<State<ModemStatus>> {
    let modem_status: Result<ModemStatus> = async {
        let modem = mm
            .list_modems()
            .await?
            .into_iter()
            .next()
            .wrap_err("couldn't find a modem")?;

        let modem_info = mm.modem_info(&modem.id).await?;

        let sim_id = modem_manager::cli::get_sim_id(modem.id.as_str()).await?;
        let iccid = modem_manager::cli::get_iccid(sim_id).await?;
        mm.signal_setup(&modem.id, Duration::from_secs(10)).await?;

        let net_stats = NetStats::from_wwan0().await?;

        Ok(ModemStatus::new(
            modem.id,
            iccid,
            modem_info.imei,
            modem_info.state,
            net_stats,
        ))
    }
    .await
    .inspect_err(|e| error!("make_modem: {e}"));

    Ok(State::new(modem_status?))
}
