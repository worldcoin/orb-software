use crate::network_manager::NetworkManager;
use color_eyre::{eyre::Context, Result};
use flume::Receiver;
use oes::NetworkInterface;
use orb_backend_status_dbus::{
    types::{ConndReport, WifiNetwork, WifiProfile},
    BackendStatusProxy,
};
use orb_dogd::MetricEmitter;
use speare::mini;
use std::{sync::Arc, time::Duration};
use tokio::time;
use tracing::{info, warn};

pub struct Args<M: MetricEmitter> {
    pub nm: NetworkManager,
    pub session_bus: zbus::Connection,
    pub report_interval: Duration,
    pub metrics: Arc<M>,
}

const IFACES: &[&str] = &["eth0", "wwan0", "wlan0"];

pub async fn report<M: MetricEmitter>(ctx: mini::Ctx<Args<M>>) -> Result<()> {
    info!("starting connd report reporter");

    async {
        let active_conns_rx: Receiver<oes::ActiveConnections> =
            ctx.subscribe("active_connections")?;

        let mut interval = time::interval(ctx.report_interval);
        interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

        let mut active_conns = loop {
            match active_conns_rx.recv_async().await {
                Ok(ac) => break ac,
                Err(e) => {
                    warn!("connd report could not get initial active connections. waiting {}s and trying again. err: {e}", ctx.report_interval.as_secs());
                    time::sleep(ctx.report_interval).await;
                    continue;
                }
            }
        };

        loop {
            tokio::select! {
                Ok(acs) = active_conns_rx.recv_async() => {
                    active_conns = acs;
                }

                _ = interval.tick() => {}
            };

            let be_status = BackendStatusProxy::new(&ctx.session_bus)
                .await
                .wrap_err("Failed to create Backend Status dbus Proxy")?;

            let egress_iface = active_conns.connections.iter().find(|c|c.primary).map(|c|{
                match c.iface {
                    NetworkInterface::Ethernet => "eth0",
                    NetworkInterface::WiFi => "wlan0",
                    NetworkInterface::Cellular => "wwan0"
                }.into()
            });

            for conn in IFACES {
                let value = match &egress_iface {
                    Some(iface) if iface == *conn => 1.0,
                    _ => 0.0,
                };

                let _ = ctx.metrics.gauge("orb.platform.connd.primary_connection", value, [format!("iface:{conn}")]);
            }

            let active_wifi_profile = active_conns.connections.iter().find(|c|c.iface == NetworkInterface::WiFi).map(|c|c.name.clone());

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
