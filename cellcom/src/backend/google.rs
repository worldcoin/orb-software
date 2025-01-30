//! Google's Geolocation API. Supports location lookup using
//! cellular and WiFi network information.
//!
//! See https://developers.google.com/maps/documentation/geolocation/requests-geolocation

use eyre::eyre;
use serde::{Deserialize, Serialize};

use eyre::Result;
use tracing::{debug, error};

use crate::data::{CellularInfo, WifiNetwork};

const GOOGLE_GEOLOCATION_API_URL: &str =
    "https://www.googleapis.com/geolocation/v1/geolocate";

#[derive(Serialize, Debug)]
struct GeolocationRequest {
    #[serde(
        skip_serializing_if = "Option::is_none",
        rename = "homeMobileCountryCode"
    )]
    home_mobile_country_code: Option<u32>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        rename = "homeMobileNetworkCode"
    )]
    home_mobile_network_code: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "radioType")]
    radio_type: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", rename = "cellTowers")]
    cell_towers: Vec<CellTower>,
    #[serde(skip_serializing_if = "Vec::is_empty", rename = "wifiAccessPoints")]
    wifi_access_points: Vec<WifiAccessPoint>,
    #[serde(rename = "considerIp")]
    consider_ip: bool,
}

#[derive(Serialize, Debug)]
struct CellTower {
    #[serde(rename = "cellId")]
    cell_id: u32,
    #[serde(skip_serializing_if = "Option::is_none", rename = "locationAreaCode")]
    location_area_code: Option<String>,
    #[serde(rename = "mobileCountryCode")]
    mobile_country_code: Option<u32>,
    #[serde(rename = "mobileNetworkCode")]
    mobile_network_code: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    age: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "signalStrength")]
    signal_strength: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "timingAdvance")]
    timing_advance: Option<u32>,
}

#[derive(Serialize, Debug)]
struct WifiAccessPoint {
    #[serde(rename = "macAddress")]
    mac_address: String,
    #[serde(rename = "signalStrength")]
    signal_strength: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    age: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    channel: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "signalToNoiseRatio")]
    signal_to_noise_ratio: Option<i32>,
}

#[derive(Serialize, Deserialize)]
pub struct Location {
    pub lat: f64,
    pub lng: f64,
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
pub enum GeolocationResponse {
    Success { location: Location, accuracy: f64 },
    Error { error: GoogleError },
}

#[derive(Serialize, Deserialize)]
pub struct GoogleError {
    pub code: i32,
    pub message: String,
    pub status: String,
}

pub fn get_location(
    api_key: &str,
    cellular_info: &CellularInfo,
    wifi_networks: &[WifiNetwork],
) -> Result<GeolocationResponse> {
    let request = build_geolocation_request(cellular_info, wifi_networks)?;

    debug!(
        request = ?serde_json::to_string(&request)?,
        "Sending geolocation request"
    );

    let client = reqwest::blocking::Client::new();
    let response_raw = client
        .post(format!("{}?key={}", GOOGLE_GEOLOCATION_API_URL, api_key))
        .json(&request)
        .send()?;

    let response_text = response_raw.text()?;
    let response: GeolocationResponse =
        serde_json::from_str(&response_text).map_err(|e| {
            error!("Failed to parse response: {}", e);
            error!("Raw response: {}", response_text);
            e
        })?;

    match response {
        GeolocationResponse::Success { .. } => Ok(response),
        GeolocationResponse::Error { error } => Err(eyre!(
            "Google API error: {} ({})",
            error.message,
            error.code
        )),
    }
}

fn build_geolocation_request(
    cellular_info: &CellularInfo,
    wifi_networks: &[WifiNetwork],
) -> Result<GeolocationRequest> {
    let serving_cell = &cellular_info.serving_cell;

    let cell_towers = vec![CellTower {
        cell_id: u32::from_str_radix(&serving_cell.cell_id, 16)?,
        location_area_code: None,
        mobile_country_code: serving_cell.mcc,
        mobile_network_code: serving_cell.mnc,
        age: None,
        signal_strength: serving_cell.rssi,
        timing_advance: None,
    }];

    let wifi_access_points: Vec<WifiAccessPoint> = wifi_networks
        .iter()
        .map(|network| WifiAccessPoint {
            mac_address: network.bssid.clone(),
            signal_strength: network.signal_level,
            age: None,
            channel: Some(network.frequency),
            signal_to_noise_ratio: None,
        })
        .collect();

    Ok(GeolocationRequest {
        home_mobile_country_code: serving_cell.mcc,
        home_mobile_network_code: serving_cell.mnc,
        radio_type: Some(serving_cell.network_type.clone()),
        cell_towers,
        wifi_access_points,
        consider_ip: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cell::data::ServingCell;

    #[test]
    fn test_build_geolocation_request_valid() {
        let cellular_info = CellularInfo {
            serving_cell: ServingCell {
                connection_status: "CONNECT".to_string(),
                network_type: "LTE".to_string(),
                duplex_mode: "FDD".to_string(),
                mcc: Some(310),
                mnc: Some(260),
                cell_id: "00AB12".to_string(), // hex
                channel_or_arfcn: Some(100),
                pcid_or_psc: Some(22),
                rsrp: Some(-90),
                rsrq: Some(-10),
                rssi: Some(-60),
                sinr: Some(12),
            },
            neighbor_cells: vec![], // not used in build_geolocation_request
        };

        let wifi_networks = vec![WifiNetwork {
            bssid: "00:11:22:33:44:55".into(),
            frequency: 2412,
            signal_level: -45,
            flags: "[WPA2-PSK-CCMP][ESS]".into(),
            ssid: "TestAP".into(),
        }];

        let req = build_geolocation_request(&cellular_info, &wifi_networks).unwrap();

        assert_eq!(req.cell_towers.len(), 1);
        assert_eq!(req.wifi_access_points.len(), 1);

        let tower = &req.cell_towers[0];

        // 0x00AB12 => 70130 decimal
        assert_eq!(tower.cell_id, 0x00AB12);
        assert_eq!(tower.mobile_country_code, Some(310));
        assert_eq!(tower.mobile_network_code, Some(260));

        let ap = &req.wifi_access_points[0];
        assert_eq!(ap.mac_address, "00:11:22:33:44:55");
        assert_eq!(ap.signal_strength, -45);
        assert_eq!(ap.channel, Some(2412));
    }

    #[test]
    fn test_build_geolocation_request_invalid_hex() {
        let cellular_info = CellularInfo {
            serving_cell: ServingCell {
                connection_status: "CONNECT".to_string(),
                network_type: "LTE".to_string(),
                duplex_mode: "FDD".to_string(),
                mcc: Some(310),
                mnc: Some(260),
                cell_id: "GARBAGE".to_string(),
                channel_or_arfcn: None,
                pcid_or_psc: None,
                rsrp: None,
                rsrq: None,
                rssi: None,
                sinr: None,
            },
            neighbor_cells: vec![],
        };

        let wifi_networks = vec![];
        let err =
            build_geolocation_request(&cellular_info, &wifi_networks).unwrap_err();

        assert!(err.to_string().contains("invalid digit"));
    }
}
