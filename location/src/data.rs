use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct NetworkInfo {
    pub wifi: Vec<WifiNetwork>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WifiNetwork {
    pub bssid: String,
    pub frequency: u32,
    pub signal_level: i32,
    pub flags: String,
    pub ssid: String,
}
