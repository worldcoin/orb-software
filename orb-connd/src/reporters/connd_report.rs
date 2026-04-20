use crate::network_manager::{Connection, NetworkManager};
use color_eyre::{eyre::Context, Result};
use flume::Receiver;
use orb_backend_status_dbus::{
    types::{ConndReport, WifiNetwork, WifiProfile},
    BackendStatusProxy,
};
use speare::mini;
use std::time::Duration;
use tokio::time;
use tracing::{info, warn};

pub struct Args {
    pub nm: NetworkManager,
    pub session_bus: zbus::Connection,
    pub report_interval: Duration,
}

pub async fn report(ctx: mini::Ctx<Args>) -> Result<()> {
    info!("starting connd report reporter");

    async {
        let active_conns_rx: Receiver<oes::ActiveConnections> =
            ctx.subscribe("active_connections")?;

        let mut interval = time::interval(ctx.report_interval);
        interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                Ok(_) = active_conns_rx.recv_async() => {}
                _ = interval.tick() => {}
            };

            let be_status = BackendStatusProxy::new(&ctx.session_bus)
                .await
                .wrap_err("Failed to create Backend Status dbus Proxy")?;

            let primary_conn = ctx
                .nm
                .primary_connection()
                .await
                .inspect_err(|e| warn!("failed to get primary connection: {e}"))
                .unwrap_or_default();

            let (egress_iface, active_wifi_profile) = match primary_conn {
                Some(Connection::Cellular { .. }) => (Some("wwan0".into()), None),
                Some(Connection::Wifi { ssid }) => (Some("wlan0".into()), Some(ssid)),
                Some(Connection::Ethernet) => (Some("eth0".into()), None),
                None => (None, None),
            };

            let saved_wifi_profiles = ctx
                .nm
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

            let scanned_networks: Vec<WifiNetwork> = ctx
                .nm
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

            be_status
                .provide_connd_report(ConndReport {
                    egress_iface,
                    wifi_enabled: ctx.nm.wifi_enabled().await?,
                    smart_switching: ctx.nm.smart_switching_enabled().await?,
                    airplane_mode: false, // not implemented yet
                    active_wifi_profile,
                    saved_wifi_profiles,
                    scanned_networks,
                })
                .await?;
        }

        #[allow(unreachable_code)]
        Ok::<(), color_eyre::Report>(())
    }
    .await
    .inspect_err(|e| warn!("failed to provide connd report to backend status: {e}"))
}
