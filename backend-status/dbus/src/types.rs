use zbus::zvariant::{DeserializeDict, SerializeDict, Type};

#[derive(Debug, Clone, SerializeDict, DeserializeDict, Type, Eq, PartialEq)]
#[zvariant(signature = "a{sv}")]
pub struct WifiNetwork {
    #[zvariant(rename = "id")]
    pub bssid: String,
    #[zvariant(rename = "fr")]
    pub frequency: u32,
    #[zvariant(rename = "sl")]
    pub signal_level: i32,
    #[zvariant(rename = "fl")]
    pub flags: String,
    #[zvariant(rename = "ss")]
    pub ssid: String,
}

#[derive(Debug, Clone, SerializeDict, DeserializeDict, Type, Eq, PartialEq)]
#[zvariant(signature = "a{sv}")]
pub struct UpdateProgress {
    #[zvariant(rename = "dp")]
    pub download_progress: u64,
    #[zvariant(rename = "pp")]
    pub processed_progress: u64,
    #[zvariant(rename = "ip")]
    pub install_progress: u64,
    #[zvariant(rename = "tp")]
    pub total_progress: u64,
    #[zvariant(rename = "er")]
    pub error: Option<String>,
}

pub const COMPLETED_PROGRESS: u64 = 100;

impl UpdateProgress {
    pub fn completed() -> Self {
        Self {
            download_progress: COMPLETED_PROGRESS,
            processed_progress: COMPLETED_PROGRESS,
            install_progress: COMPLETED_PROGRESS,
            total_progress: COMPLETED_PROGRESS,
            error: None,
        }
    }
}

#[derive(Debug, Clone, SerializeDict, DeserializeDict, Type, Eq, PartialEq)]
#[zvariant(signature = "a{sv}")]
pub struct NetStats {
    #[zvariant(rename = "intfs")]
    pub interfaces: Vec<NetIntf>,
}

#[derive(Debug, Clone, SerializeDict, DeserializeDict, Type, Eq, PartialEq)]
#[zvariant(signature = "a{sv}")]
pub struct NetIntf {
    pub name: String,
    pub tx_bytes: u64,
    pub rx_bytes: u64,
    pub tx_packets: u64,
    pub rx_packets: u64,
    pub tx_errors: u64,
    pub rx_errors: u64,
}
