use crate::{
    mcu_util::{McuUtil, Module},
    modem_manager::{
        connection_state::ConnectionState, Location, ModemId, ModemManager, Signal,
    },
    systemd::Systemd,
};
use color_eyre::{
    eyre::{eyre, Context, ContextCompat},
    Result,
};
use speare::mini;
use std::{sync::Arc, time::Duration};
use tokio::{
    fs,
    time::{self, timeout},
};
use tracing::{error, info, warn};

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub id: ModemId,
    pub fw_revision: Option<String>,
    pub iccid: Option<String>,
    pub imei: String,
    pub rat: Option<String>,
    pub operator: Option<String>,
    pub state: ConnectionState,
    pub signal: Signal,
    pub location: Location,
}

pub struct Args {
    pub poll_interval: Duration,
    pub modem_manager: Arc<dyn ModemManager>,
    pub mcu_util: Arc<dyn McuUtil>,
    pub systemd: Systemd,
}

pub async fn supervisor(ctx: mini::Ctx<Args>) -> Result<()> {
    info!("starting modem supervisor");

    let mut snapshot: Option<Snapshot> = None;
    let mut refresh_snapshot = async || -> Result<()> {
        let new_snapshot = take_snapshot(ctx.modem_manager.as_ref()).await?;

        let modem_id_changed_msg = match &snapshot {
            None => Some(format!(
                "modem detected with id {}",
                new_snapshot.id.as_str()
            )),

            Some(snap) if snap.id != new_snapshot.id => Some(format!(
                "modem changed id from {} to {}",
                snap.id.as_str(),
                new_snapshot.id.as_str()
            )),

            _ => None,
        };

        if let Some(msg) = modem_id_changed_msg {
            warn!(msg);

            let _ =
                setup_signal_and_bands(ctx.modem_manager.as_ref(), &new_snapshot.id)
                    .await
                    .inspect_err(|e| warn!("failed to setup signal and bands: {e:?}"));
        }

        let _ = ctx.publish("modem-snapshot", new_snapshot.clone());

        snapshot = Some(new_snapshot);

        Ok(())
    };

    let mut update_interval = time::interval(ctx.poll_interval);

    loop {
        if let Err(e) = refresh_snapshot().await {
            error!("failed to refresh modem snapshot with err: {e}");
            error!("powercycling modem");

            let _ = powercycle_modem(ctx.mcu_util.as_ref(), &ctx.systemd)
                .await
                .inspect_err(|e| {
                    error!("failed to to powercycle modem with err: {e:?}");
                });

            return Err(e);
        }

        update_interval.tick().await;
    }
}

async fn take_snapshot(mm: &dyn ModemManager) -> Result<Snapshot> {
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

    let signal = mm
        .signal_get(&modem.id)
        .await
        .inspect_err(|e| warn!("failed to retrieve modem signal info: {e}"))
        .unwrap_or_default();

    let location = mm
        .location_get(&modem.id)
        .await
        .inspect_err(|e| warn!("failed to retrieve modem location info: {e}"))
        .unwrap_or_default();

    Ok(Snapshot {
        id: modem.id,
        fw_revision: modem_info.fw_revision,
        iccid,
        imei: modem_info.imei,
        rat: modem_info.access_tech,
        operator: modem_info.operator_name,
        state: modem_info.state,
        signal,
        location,
    })
}

async fn setup_signal_and_bands(mm: &dyn ModemManager, id: &ModemId) -> Result<()> {
    mm.signal_setup(id, std::time::Duration::from_secs(10))
        .await
        .map_err(|e| eyre!("could not update modem signal refresh rate: {e}"))?;

    mm.set_current_bands(id, &ALLOWED_BANDS)
        .await
        .map_err(|e| eyre!("could not set modem bands: {e}"))?;

    Ok(())
}

static ALLOWED_BANDS: [&str; 27] = [
    "egsm",
    "dcs",
    "pcs",
    "g850",
    "utran-1",
    "utran-2",
    "utran-4",
    "utran-5",
    "utran-6",
    "utran-8",
    "eutran-1",
    "eutran-2",
    "eutran-3",
    "eutran-4",
    "eutran-5",
    "eutran-7",
    "eutran-8",
    "eutran-9",
    "eutran-12",
    "eutran-13",
    "eutran-14",
    "eutran-18",
    "eutran-19",
    "eutran-20",
    "eutran-25",
    "eutran-26",
    "eutran-28",
];

async fn powercycle_modem(mcu_util: &dyn McuUtil, systemd: &Systemd) -> Result<()> {
    mcu_util
        .powercycle(Module::Modem)
        .await
        .wrap_err("mcu-util power-cycle")?;

    time::sleep(Duration::from_secs(5)).await;

    let device_exists = async {
        loop {
            if fs::try_exists("/dev/cdc-wdm0").await.is_ok_and(|x| x) {
                break;
            }

            time::sleep(Duration::from_secs(1)).await;
        }
    };

    timeout(Duration::from_secs(30), device_exists)
        .await
        .wrap_err("timed out after 30s waiting for modem device to pop back up")?;

    info!("modem detected at /dev/cdc-wdm0");

    systemd
        .restart_service("ModemManager.service")
        .await
        .wrap_err("restart ModemManager systemd service")?;

    info!("ModemManager restarted!");

    Ok(())
}
