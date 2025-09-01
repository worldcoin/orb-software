use crate::{
    telemetry::modem::Modem,
    utils::{retry_for, State},
};
use color_eyre::{eyre::eyre, Result};
use orb_backend_status_dbus::{types::CellularStatus, BackendStatusProxy};
use std::time::Duration;
use tokio::{
    task::{self, JoinHandle},
    time,
};
use tracing::{error, info};
use zbus::Connection;

pub fn start(modem: State<Modem>, report_interval: Duration) -> JoinHandle<Result<()>> {
    info!("starting backend status reporter");
    task::spawn(async move {
        let be_status: BackendStatusProxy<'_> =
            retry_for(Duration::MAX, Duration::from_secs(20), make_backend_status)
                .await?;

        info!("successfully created BackendStatusProxy");

        loop {
            if let Err(e) = report(&modem, &be_status).await {
                error!("failed to report to backend status: {e}");
            }

            time::sleep(report_interval).await;
        }
    })
}

async fn report(
    modem: &State<Modem>,
    be_status: &BackendStatusProxy<'_>,
) -> Result<()> {
    let cellular_status: CellularStatus = modem
        .read(|m| {
            let signal = m.signal.as_ref();

            CellularStatus {
                imei: m.imei.clone(),
                iccid: m.iccid.clone(),
                rat: m.rat.clone(),
                operator: m.operator.clone(),
                rsrp: signal.and_then(|s| s.rsrp),
                rsrq: signal.and_then(|s| s.rsrq),
                rssi: signal.and_then(|s| s.rssi),
                snr: signal.and_then(|s| s.snr),
            }
        })
        .map_err(|e| {
            eyre!("failed to read ConnectionState from State<Modem>: {e:?}")
        })?;

    be_status.provide_cellular_status(cellular_status).await?;

    Ok(())
}

async fn make_backend_status() -> Result<BackendStatusProxy<'static>> {
    let conn = Connection::session()
        .await
        .inspect_err(|e| error!("Failed to initialize dbus session: {e}"))?;

    let proxy = BackendStatusProxy::new(&conn).await.inspect_err(|e| {
        error!("Failed to connect to Backend Status dbus Proxy: {e}")
    })?;

    Ok(proxy)
}
