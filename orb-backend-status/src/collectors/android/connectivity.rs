use super::ReporterState;
use crate::{
    backend::types::{WifiApiV2, WifiQualityApiV2},
    collectors::GlobalConnectivity,
    orb_event_stream::Event,
};
use serde::Deserialize;
use tracing::warn;

pub(super) fn active_connections(event: &Event) -> Option<oes::ActiveConnections> {
    if event.name != "raven/active_connections" {
        return None;
    }
    let payload = event.payload.as_ref().unwrap_or(&serde_json::Value::Null);
    oes::ActiveConnections::deserialize(payload)
        .inspect_err(|_| warn!("invalid raven/active_connections payload"))
        .ok()
}

fn wifi_connection(report: oes::ActiveConnections) -> Option<oes::Connection> {
    report
        .connections
        .into_iter()
        .find(|c| c.iface == oes::NetworkInterface::WiFi)
}

pub(super) fn update_connectivity(
    state: &ReporterState,
    report: oes::ActiveConnections,
) -> bool {
    let online = report
        .connections
        .iter()
        .any(|c| c.primary && c.has_internet);
    let next = if online {
        GlobalConnectivity::Connected {
            ssid: wifi_connection(report).map(|wifi| wifi.name),
        }
    } else {
        GlobalConnectivity::NotConnected
    };
    let changed = state.connectivity_tx.send_if_modified(|previous| {
        if *previous == next {
            return false;
        }
        *previous = next;
        true
    });
    online && changed
}

pub(crate) fn wifi_report(events: &[Event]) -> Option<WifiApiV2> {
    let wifi = wifi_connection(events.iter().find_map(active_connections)?)?;

    let ssid = (!wifi.name.is_empty()).then_some(wifi.name);
    if ssid.is_none()
        && wifi.bssid.is_none()
        && wifi.frequency_mhz.is_none()
        && wifi.rssi_dbm.is_none()
    {
        return None;
    }

    Some(WifiApiV2 {
        ssid,
        bssid: wifi.bssid,
        frequency: wifi.frequency_mhz,
        quality: wifi.rssi_dbm.map(|rssi| WifiQualityApiV2 {
            signal_level: Some(rssi),
            bit_rate: None,
            link_quality: None,
            noise_level: None,
        }),
    })
}
