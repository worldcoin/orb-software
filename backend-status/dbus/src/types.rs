use zbus::zvariant::{DeserializeDict, SerializeDict, Type};

#[derive(Debug, Clone, SerializeDict, DeserializeDict, Type)]
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

#[derive(Debug, Clone, SerializeDict, DeserializeDict, Type)]
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
