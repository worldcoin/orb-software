use serde::{Deserialize, Serialize};
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

/// All Option<T> fields make use of the `option-as-array` features of zbus.
/// https://dbus2.github.io/zbus/faq.html#2-encoding-as-an-array-a
#[derive(Debug, Clone, Type, Serialize, Deserialize, PartialEq)]
pub struct LteInfo {
    imei: String,
    iccid: String,
    /// Radio Access Technology -- e.g.: gsm, lte
    rat: Option<String>,
    operator: Option<String>,
    /// Reference Option Received Power — how strong the LTE signal is.
    rsrp: Option<f64>,
    ///Reference Signal Received Quality — signal quality, affected by interference.
    rsrq: Option<f64>,
    /// Received Signal Strength Indicator — total signal power (including noise)
    rssi: Option<f64>,
    /// Signal-to-Noise Ratio — how "clean" the signal is.
    snr: Option<f64>,
}
