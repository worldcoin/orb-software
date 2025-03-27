use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrbStatusV2 {
    pub orb_id: Option<String>,
    pub location_data: Option<LocationDataV2>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocationDataV2 {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wifi: Option<Vec<WifiDataV2>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cell: Option<Vec<CellDataV2>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WifiDataV2 {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bssid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signal_strength: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signal_to_noise_ratio: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CellDataV2 {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcc: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mnc: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lac: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cell_id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signal_strength: Option<i32>,
}
