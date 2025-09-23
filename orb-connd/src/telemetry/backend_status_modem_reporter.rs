use crate::{telemetry::modem_status::ModemStatus, utils::State};
use color_eyre::{eyre::eyre, Result};
use orb_backend_status_dbus::{types::CellularStatus, BackendStatusProxy};
use std::time::Duration;
use tokio::{
    task::{self, JoinHandle},
    time,
};
use tracing::{error, info};

pub fn spawn(
    conn: zbus::Connection,
    modem: State<ModemStatus>,
    report_interval: Duration,
) -> JoinHandle<Result<()>> {
    info!("starting backend status reporter");
    task::spawn(async move {
        loop {
            if let Err(e) = report(&conn, &modem).await {
                error!("failed to report to backend status: {e}");
            }

            time::sleep(report_interval).await;
        }
    })
}

async fn report(conn: &zbus::Connection, modem: &State<ModemStatus>) -> Result<()> {
    let be_status = BackendStatusProxy::new(conn)
        .await
        .inspect_err(|e| error!("Failed to create Backend Status dbus Proxy: {e}"))?;

    let cellular_status: CellularStatus = modem
        .read(|m| {
            let signal = &m.signal;

            CellularStatus {
                imei: m.imei.clone(),
                iccid: m.iccid.clone(),
                rat: m.rat.clone(),
                operator: m.operator.clone(),
                rsrp: signal.rsrp,
                rsrq: signal.rsrq,
                rssi: signal.rssi,
                snr: signal.snr,
            }
        })
        .map_err(|e| {
            eyre!("failed to read ConnectionState from State<Modem>: {e:?}")
        })?;

    be_status.provide_cellular_status(cellular_status).await?;

    Ok(())
}
