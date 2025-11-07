use super::modem_status::ModemStatus;
use crate::modem_manager::ModemManager;
use crate::telemetry::net_stats::NetStats;
use crate::utils::State;
use color_eyre::eyre::{eyre, ContextCompat};
use color_eyre::Result;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::task::{self, JoinHandle};
use tokio::time::{self};
use tracing::{error, info};

pub fn spawn(
    mm: Arc<dyn ModemManager>,
    modem: State<ModemStatus>,
    sysfs: PathBuf,
    poll_interval: Duration,
) -> JoinHandle<Result<()>> {
    info!("starting modem monitor");

    task::spawn(async move {
        loop {
            if let Err(e) = update_modem(&mm, &modem, &sysfs).await {
                error!("failed to update modem: {e}");
            }

            time::sleep(poll_interval).await;
        }
    })
}

async fn update_modem(
    mm: &Arc<dyn ModemManager>,
    modem_status: &State<ModemStatus>,
    sysfs: impl AsRef<Path>,
) -> Result<()> {
    let current_modem_id = modem_status.read(|ms| ms.id.clone()).map_err(|e| {
        eyre!("failed to read ConnectionState from State<ModemStatus>: {e:?}")
    })?;

    let modem = mm
        .list_modems()
        .await?
        .into_iter()
        .next()
        .wrap_err("could not find a modem")?;

    // modem has most likely power cycled, enable signals refresh again
    if modem.id != current_modem_id {
        mm.signal_setup(&modem.id, Duration::from_secs(10)).await?;
    }

    let modem_info = mm.modem_info(&modem.id).await?;
    let signal = mm.signal_get(&modem.id).await?;
    let location = mm.location_get(&modem.id).await?;

    let net_stats = NetStats::collect(sysfs, "wwan0")
        .await
        .inspect_err(|e| error!("NetStats from wwan0: err {e}"));

    modem_status
        .write(move |ms| {
            ms.id = modem.id;
            ms.state = modem_info.state;
            ms.rat = modem_info.access_tech;
            ms.operator = modem_info.operator_name;
            ms.signal = signal;
            ms.location = location;

            if let Ok(stats) = net_stats {
                ms.net_stats = stats;
            }
        })
        .map_err(|e| eyre!("failed to write to State<ModemStatus>: {e:?}"))?;

    Ok(())
}
