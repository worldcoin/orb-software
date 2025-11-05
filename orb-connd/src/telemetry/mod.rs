use crate::{
    modem_manager::ModemManager,
    statsd::StatsdClient,
    telemetry::modem_status::ModemStatus,
    utils::{retry_for, State},
    OrbCapabilities, Tasks,
};
use color_eyre::{eyre::ContextCompat, Result};
use net_stats::NetStats;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tracing::{error, info};

pub mod backend_status_cellular_reporter;
pub mod backend_status_wifi_reporter;
pub mod dd_modem_reporter;
pub mod modem_monitor;
pub mod modem_status;
pub mod net_stats;

pub async fn spawn(
    system_bus: zbus::Connection,
    session_bus: zbus::Connection,
    modem_manager: Arc<dyn ModemManager>,
    statsd_client: impl StatsdClient,
    sysfs: PathBuf,
    cap: OrbCapabilities,
) -> Result<Tasks> {
    info!("starting telemetry task");
    info!("getting initial modem information");

    let mut tasks = vec![];

    if let OrbCapabilities::CellularAndWifi = cap {
        let modem_status =
            retry_for(Duration::from_secs(120), Duration::from_secs(10), || {
                make_modem_status(&modem_manager, &sysfs)
            })
            .await?;

        tasks.extend([
            modem_monitor::spawn(
                modem_manager,
                modem_status.clone(),
                sysfs,
                Duration::from_secs(20),
            ),
            backend_status_cellular_reporter::spawn(
                session_bus.clone(),
                modem_status.clone(),
                Duration::from_secs(30),
            ),
            dd_modem_reporter::spawn(
                modem_status,
                statsd_client,
                Duration::from_secs(20),
            ),
        ]);
    }

    tasks.push(backend_status_wifi_reporter::spawn(
        system_bus,
        session_bus,
        Duration::from_secs(30),
    ));

    Ok(tasks)
}

async fn make_modem_status(
    mm: &Arc<dyn ModemManager>,
    sysfs: impl AsRef<Path>,
) -> Result<State<ModemStatus>> {
    let modem_status: Result<ModemStatus> = async {
        let modem = mm
            .list_modems()
            .await?
            .into_iter()
            .next()
            .wrap_err("couldn't find a modem")?;

        let modem_info = mm.modem_info(&modem.id).await?;

        let iccid = match modem_info.sim {
            None => None,
            Some(sim_id) => {
                let sim_info = mm.sim_info(&sim_id).await?;

                Some(sim_info.iccid)
            }
        };

        mm.signal_setup(&modem.id, Duration::from_secs(10)).await?;

        let net_stats = NetStats::collect(sysfs, "wwan0").await?;

        Ok(ModemStatus::new(
            modem.id,
            iccid,
            modem_info.imei,
            modem_info.state,
            net_stats,
        ))
    }
    .await
    .inspect_err(|e| error!("make_modem_status: {e}"));

    Ok(State::new(modem_status?))
}
