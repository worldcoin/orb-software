use crate::modem;
use color_eyre::{
    eyre::{eyre, Context},
    Result,
};
use orb_backend_status_dbus::{types::CellularStatus, BackendStatusProxy};
use speare::mini;
use tracing::{error, info};

pub struct Args {
    pub dbus: zbus::Connection,
    pub zsender: zenorb::Sender,
}

pub async fn report(ctx: mini::Ctx<Args>) -> Result<()> {
    info!("starting cellular status reporter");
    let run = async || -> Result<()> {
        let snapshot_rx = ctx.subscribe::<modem::Snapshot>("modem-snapshot")?;

        loop {
            let modem = snapshot_rx.recv_async().await?;
            let signal = modem.signal;

            let be_status = BackendStatusProxy::new(&ctx.dbus)
                .await
                .wrap_err("Failed to create Backend Status dbus Proxy")?;

            // TODO: move this to oes crate once we deprecate keeping this in backend-status state
            let cell_status = CellularStatus {
                imei: modem.imei.clone(),
                iccid: modem.iccid.clone(),
                rat: modem.rat.clone(),
                operator: modem.operator.clone(),
                rsrp: signal.rsrp,
                rsrq: signal.rsrq,
                rssi: signal.rssi,
                snr: signal.snr,
            };

            let payload = serde_json::to_string(&cell_status)
                .wrap_err("failed to serialize CellularStatus")?;

            let zenoh_err = ctx
                .zsender
                .publisher("oes/cellular_status")?
                .put(payload)
                .await
                .map_err(|e| {
                    eyre!("failed to send oes/cellular_status zenoh payload, err: {e}")
                });

            let dbus_err = be_status.provide_cellular_status(cell_status).await;

            zenoh_err?;
            dbus_err?;
        }
    };

    run()
        .await
        .inspect_err(|e| error!("backend status cellular reporter failed with: {e:?}"))
}
