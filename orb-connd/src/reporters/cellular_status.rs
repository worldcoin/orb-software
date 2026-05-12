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

            let dbus_cell_status = CellularStatus {
                imei: modem.imei,
                fw_revision: modem.fw_revision,
                iccid: modem.iccid,
                rat: modem.rat,
                operator: modem.operator,
                rsrp: signal.rsrp,
                rsrq: signal.rsrq,
                rssi: signal.rssi,
                snr: signal.snr,
            };

            let oes_cell_status =
                dbus_cellular_status_to_oes_cellular_status(&dbus_cell_status);

            let payload = serde_json::to_string(&oes_cell_status)
                .wrap_err("failed to serialize CellularStatus")?;

            let zenoh_err = ctx
                .zsender
                .publisher("oes/cellular_status")?
                .put(payload)
                .await
                .map_err(|e| {
                    eyre!("failed to send oes/cellular_status zenoh payload, err: {e}")
                });

            let dbus_err = be_status.provide_cellular_status(dbus_cell_status).await;

            zenoh_err?;
            dbus_err?;
        }
    };

    run()
        .await
        .inspect_err(|e| error!("backend status cellular reporter failed with: {e:?}"))
}

fn dbus_cellular_status_to_oes_cellular_status(
    cs: &CellularStatus,
) -> oes::CellularStatus {
    oes::CellularStatus {
        imei: cs.imei.clone(),
        fw_revision: cs.fw_revision.clone(),
        iccid: cs.iccid.clone(),
        rat: cs.rat.clone(),
        operator: cs.operator.clone(),
        rsrp: cs.rsrp,
        rsrq: cs.rsrq,
        rssi: cs.rssi,
        snr: cs.snr,
    }
}
