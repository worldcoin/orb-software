use crate::lte_data::LteStat;
use color_eyre::Result;
use orb_backend_status_dbus::{types, BackendStatusProxy};
use zbus::Connection;

pub struct BackendStatus {
    proxy: BackendStatusProxy<'static>,
}

impl BackendStatus {
    pub async fn connect() -> Result<Self> {
        let conn = Connection::system().await?;
        let proxy = BackendStatusProxy::new(&conn).await?;
        Ok(Self { proxy })
    }

    pub async fn send_lte_info(
        &self,
        imei: &str,
        iccid: &str,
        rat: Option<&str>,
        operator: Option<&str>,
        snap: &LteStat,
    ) -> Result<()> {
        let lte_info = types::LteInfo {
            imei: imei.to_string(),
            iccid: iccid.to_string(),
            rat: rat.map(|s| s.to_string()),
            operator: operator.map(|s| s.to_string()),
            rsrp: snap.signal.as_ref().and_then(|s| s.rsrp),
            rsrq: snap.signal.as_ref().and_then(|s| s.rsrq),
            rssi: snap.signal.as_ref().and_then(|s| s.rssi),
            snr: snap.signal.as_ref().and_then(|s| s.snr),
        };

        let _ = self.proxy.provide_lte_info(lte_info).await;
        Ok(())
    }
}
