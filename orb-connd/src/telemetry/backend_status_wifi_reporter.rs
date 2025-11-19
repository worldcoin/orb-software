use crate::network_manager::{Connection, NetworkManager};
use color_eyre::{eyre::Context, Result};
use futures::StreamExt;
use orb_backend_status_dbus::{
    types::{ConndReport, WifiNetwork, WifiProfile},
    BackendStatusProxy,
};
use rusty_network_manager::NetworkManagerProxy;
use std::time::Duration;
use tokio::{
    task::{self, JoinHandle},
    time,
};
use tracing::{debug, error, info, warn};

pub fn spawn(
    nm: NetworkManager,
    session_bus: zbus::Connection,
    report_interval: Duration,
) -> JoinHandle<Result<()>> {
    info!("starting backend status wifi reporter");
    task::spawn(async move {
        if let Err(e) = run_reporter(nm, session_bus, report_interval).await {
            error!("wifi reporter task failed: {e}");
        }

        Ok(())
    })
}

async fn run_reporter(
    nm: NetworkManager,
    session_bus: zbus::Connection,
    report_interval: Duration,
) -> Result<()> {
    let system_bus = zbus::Connection::system()
        .await
        .wrap_err("Failed to connect to system bus")?;

    let nm_proxy = NetworkManagerProxy::new(&system_bus)
        .await
        .wrap_err("Failed to create NetworkManager proxy")?;

    let mut state_stream = nm_proxy
        .receive_state_changed()
        .await
        .wrap_err("Failed to subscribe to StateChanged signal")?;

    let mut primary_conn_stream = nm_proxy.receive_primary_connection_changed().await;

    let mut interval = time::interval(report_interval);
    interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            _ = state_stream.next() => {
                info!("NetworkManager state changed - sending immediate WiFi status");
                if let Err(e) = report(&nm, &session_bus).await {
                    error!("failed to report to backend status on state change: {e}");
                }
            }
            _ = primary_conn_stream.next() => {
                info!("Primary connection changed - sending immediate WiFi status");
                if let Err(e) = report(&nm, &session_bus).await {
                    error!("failed to report to backend status on connection change: {e}");
                }
            }
            _ = interval.tick() => {
                debug!("Periodic WiFi status report ({}s interval)", report_interval.as_secs());
                if let Err(e) = report(&nm, &session_bus).await {
                    error!("failed to report to backend status on periodic check: {e}");
                }
            }
        }
    }
}

async fn report(nm: &NetworkManager, session_bus: &zbus::Connection) -> Result<()> {
    let be_status = BackendStatusProxy::new(session_bus)
        .await
        .wrap_err("Failed to create Backend Status dbus Proxy")?;

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
        .await
        .inspect_err(|e| warn!("failed to list wifi profiles: {e}"))
        .unwrap_or_default()
        .into_iter()
        .map(|profile| WifiProfile {
            ssid: profile.ssid,
            sec: profile.sec.to_string(),
        })
        .collect();

    let scanned_networks: Vec<WifiNetwork> = nm
        .wifi_scan()
        .await
        .inspect_err(|e| warn!("failed to scan wifi: {e}"))
        .unwrap_or_default()
        .into_iter()
        .map(|ap| WifiNetwork {
            bssid: ap.bssid,
            ssid: ap.ssid,
            frequency: ap.freq_mhz,
            signal_level: ap.rssi.unwrap_or_default(),
        })
        .collect();

    let _ = async {
        be_status
            .provide_connd_report(ConndReport {
                egress_iface,
                wifi_enabled: nm.wifi_enabled().await?,
                smart_switching: nm.smart_switching_enabled().await?,
                airplane_mode: false, // not implemented yet
                active_wifi_profile,
                saved_wifi_profiles,
                scanned_networks,
            })
            .await?;

        Ok::<(), color_eyre::Report>(())
    }
    .await
    .inspect_err(|e| warn!("failed to provide connd report to backend status: {e}"));

    Ok(())
}
