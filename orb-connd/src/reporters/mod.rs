use crate::{
    modem_manager::ModemManager,
    network_manager::NetworkManager,
    reporters::modem_status::ModemStatus,
    resolved::Resolved,
    statsd::StatsdClient,
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
use tracing::{error, info, warn};

pub mod active_connections_report;
pub mod backend_status_cellular_reporter;
pub mod backend_status_wifi_reporter;
pub mod dd_modem_reporter;
pub mod modem_monitor;
pub mod modem_status;
pub mod net_changed_reporter;
pub mod net_stats;

#[allow(clippy::too_many_arguments)]
pub async fn spawn(
    nm: NetworkManager,
    resolved: Resolved,
    session_bus: zbus::Connection,
    modem_manager: Arc<dyn ModemManager>,
    statsd_client: impl StatsdClient,
    sysfs: PathBuf,
    cap: OrbCapabilities,
    zsender: zenorb::Sender,
) -> Tasks {
    info!("starting reporter tasks");

    let (health_tx, health_rx) = flume::unbounded();

    let mut tasks = vec![
        backend_status_wifi_reporter::spawn(
            nm.clone(),
            session_bus.clone(),
            Duration::from_secs(30),
        ),
        net_changed_reporter::spawn(nm.clone(), zsender.clone(), health_tx),
        active_connections_report::spawn(nm, resolved, health_rx, zsender),
    ];

    if let OrbCapabilities::CellularAndWifi = cap {
        info!("reporter getting initial modem information");
        let modem_status_timeout = Duration::from_secs(120);
        let modem_status = match retry_for(
            modem_status_timeout,
            Duration::from_secs(10),
            || make_modem_status(&modem_manager, &sysfs),
        )
        .await
        {
            Ok(ms) => ms,
            Err(error) => {
                error!(?error, "could not retrieve modem_status after {}s. modem reporting will be disabled", modem_status_timeout.as_secs());
                return tasks;
            }
        };

        tasks.extend([
            modem_monitor::spawn(
                modem_manager,
                modem_status.clone(),
                sysfs,
                Duration::from_secs(20),
            ),
            backend_status_cellular_reporter::spawn(
                session_bus,
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

    tasks
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

        if let Err(e) = mm.signal_setup(&modem.id, Duration::from_secs(10)).await {
            warn!("could not update modem signal refresh rate: {e}");
        }

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
