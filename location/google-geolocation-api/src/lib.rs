//! Google's Geolocation API. Supports location lookup using
//! cellular and WiFi network information.
//!
//! See https://developers.google.com/maps/documentation/geolocation/requests-geolocation

pub mod builder;

use eyre::eyre;
use serde::{Deserialize, Serialize};

use eyre::Result;
use tracing::{debug, error};

use crate::builder::GeolocationRequestBuilder;

const GOOGLE_GEOLOCATION_API_URL: &str =
    "https://www.googleapis.com/geolocation/v1/geolocate";

#[derive(Serialize, Debug, Default)]
pub struct GeolocationRequest {
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

impl GeolocationRequest {
    pub fn builder() -> GeolocationRequestBuilder {
        GeolocationRequestBuilder::default()
    }
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
    request: &GeolocationRequest,
) -> Result<GeolocationResponse> {
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
