use chrono::{DateTime, Utc};
use orb_update_agent_dbus::UpdateAgentState;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrbStatusV2 {
    pub orb_id: Option<String>,
    pub orb_name: Option<String>,
    pub jabil_id: Option<String>,
    pub version: Option<VersionV2>,
    pub location_data: Option<LocationDataV2>,
    pub update_progress: Option<UpdateProgressV2>,
    pub net_stats: Option<NetStatsV2>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionV2 {
    pub current_release: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocationDataV2 {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wifi: Option<Vec<WifiDataV2>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cell: Option<Vec<CellDataV2>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateProgressV2 {
    pub download_progress: u64,
    pub processed_progress: u64,
    pub install_progress: u64,
    pub total_progress: u64,
    pub error: Option<String>,
    pub state: UpdateAgentState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetStatsV2 {
    pub interfaces: Vec<NetIntfV2>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetIntfV2 {
    pub name: String,
    pub tx_bytes: u64,
    pub rx_bytes: u64,
    pub tx_packets: u64,
    pub rx_packets: u64,
    pub tx_errors: u64,
    pub rx_errors: u64,
}
