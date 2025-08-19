use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrbStatusApiV2 {
    pub orb_id: Option<String>,
    pub orb_name: Option<String>,
    pub jabil_id: Option<String>,
    pub version: Option<VersionApiV2>,
    pub wifi: Option<WifiApiV2>,
    pub mac_address: Option<String>,
    pub uptime_sec: Option<f64>,
    // orb metrics
    pub battery: Option<BatteryApiV2>,
    pub timestamp: DateTime<Utc>,
    pub temperature: Option<TemperatureApiV2>,
    pub ssd: Option<SsdStatusApiV2>,
    pub update_progress: Option<UpdateProgressApiV2>,
    pub net_stats: Option<NetStatsApiV2>,
    // orb location
    pub location_data: Option<LocationDataApiV2>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatteryApiV2 {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_charging: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WifiQualityApiV2 {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bit_rate: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link_quality: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signal_level: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub noise_level: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WifiApiV2 {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bssid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality: Option<WifiQualityApiV2>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemperatureApiV2 {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub front_unit: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub front_pcb: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub battery_pcb: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub battery_cell: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup_battery: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub liquid_lens: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub main_accelerometer: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub main_mcu: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mainboard: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub security_accelerometer: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub security_mcu: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub battery_pack: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssd: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeoLocationApiV2 {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coordinates: Option<Vec<f64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latitude: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub longitude: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocationDataApiV2 {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gps: Option<GpsDataApiV2>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wifi: Option<Vec<WifiDataApiV2>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cell: Option<Vec<CellDataApiV2>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpsDataApiV2 {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latitude: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub longitude: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WifiDataApiV2 {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bssid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signal_strength: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signal_to_noise_ratio: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CellDataApiV2 {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SsdStatusApiV2 {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_left: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub space_left: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signup_left_to_upload: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionApiV2 {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_release: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateProgressApiV2 {
    pub download_progress: u64,
    pub processed_progress: u64,
    pub install_progress: u64,
    pub total_progress: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetStatsApiV2 {
    pub interfaces: Vec<NetIntfApiV2>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetIntfApiV2 {
    pub name: String,
    pub tx_bytes: u64,
    pub rx_bytes: u64,
    pub tx_packets: u64,
    pub rx_packets: u64,
    pub tx_errors: u64,
    pub rx_errors: u64,
}
