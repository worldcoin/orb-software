use crate::network_manager::{Connection, NetworkManager};
use color_eyre::{eyre::Context, Result};
use orb_backend_status_dbus::{
    types::{ConndReport, WifiProfile},
    BackendStatusProxy,
};
use std::time::Duration;
use tokio::{
    task::{self, JoinHandle},
    time,
};
use tracing::{error, info, warn};

pub fn spawn(
    system_bus: zbus::Connection,
    session_bus: zbus::Connection,
    report_interval: Duration,
) -> JoinHandle<Result<()>> {
    info!("starting backend status wifi reporter");
    task::spawn(async move {
        loop {
            if let Err(e) = report(&system_bus, &session_bus).await {
                error!("failed to report to backend status: {e}");
            }

            time::sleep(report_interval).await;
        }
    })
}

async fn report(
    system_bus: &zbus::Connection,
    session_bus: &zbus::Connection,
) -> Result<()> {
    let be_status = BackendStatusProxy::new(session_bus)
        .await
        .wrap_err("Failed to create Backend Status dbus Proxy")?;

    let nm = NetworkManager::new(system_bus.clone());
    let primary_conn = nm
        .primary_connection()
        .await
        .inspect_err(|e| warn!("failed to get primary connection: {e}"))
        .unwrap_or_default();

    let (egress_iface, active_wifi_profile) = match primary_conn {
        Some(Connection::Cellular { .. }) => (Some("wwan0".into()), None),
        Some(Connection::Wifi { ssid }) => (Some("wlan0".into()), Some(ssid)),
        None => (None, None),
    };

    let saved_wifi_profiles = nm
        .list_wifi_profiles()
        .await?
        .into_iter()
        .map(|profile| WifiProfile {
            ssid: profile.ssid,
            sec: profile.sec.to_string(),
            psk: profile.psk,
        })
        .collect();

    be_status
        .provide_connd_report(ConndReport {
            egress_iface,
            wifi_enabled: nm.wifi_enabled().await?,
            smart_switching: nm.smart_switching_enabled().await?,
            airplane_mode: false, // not implemented yet
            active_wifi_profile,
            saved_wifi_profiles,
        })
        .await?;

    Ok(())
}
