use crate::{
    backend::types::{WifiApiV2, WifiQualityApiV2},
    orb_event_stream::Event,
};
use serde::Deserialize;
use tracing::warn;

pub(crate) fn wifi_report(events: &[Event]) -> Option<WifiApiV2> {
    let event = events
        .iter()
        .find(|event| event.name == "raven/active_connections")?;

    let report = event
        .payload
        .as_ref()
        .and_then(|payload| oes::ActiveConnections::deserialize(payload).ok());

    let Some(report) = report else {
        warn!("invalid raven/active_connections payload");
        return None;
    };

    let wifi = report
        .connections
        .into_iter()
        .find(|connection| connection.iface == oes::NetworkInterface::WiFi)?;

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
