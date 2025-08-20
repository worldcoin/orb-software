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

//--------------------------------
// Core Stats
//--------------------------------

/// The JSON structure of the orb status request.
#[derive(Debug, Default, Clone, Type, Serialize, Deserialize)]
pub struct CoreStats {
    pub battery: Battery,
    pub wifi: Option<Wifi>,
    pub temperature: Temperature,
    pub location: Location,
    pub ssd: Ssd,
    pub version: OrbVersion,
    pub mac_address: String,
}

#[allow(missing_docs)]
#[derive(Debug, Clone, SerializeDict, DeserializeDict, Type)]
#[zvariant(signature = "a{sv}")]
pub struct Battery {
    pub level: f64,
    pub is_charging: bool,
}

impl Default for Battery {
    fn default() -> Self {
        // is_charging set to true prevents the charging sound to play on boot if the orb is plugged in
        Self {
            level: f64::default(),
            is_charging: true,
        }
    }
}

#[allow(missing_docs)]
#[derive(Debug, Default, Clone, SerializeDict, DeserializeDict, Type)]
#[zvariant(signature = "a{sv}")]
pub struct Wifi {
    pub ssid: String,
    pub bssid: String,
    pub quality: WifiQuality,
}

#[allow(missing_docs)]
#[derive(Debug, Default, Clone, SerializeDict, DeserializeDict, Type)]
#[zvariant(signature = "a{sv}")]
pub struct WifiQuality {
    pub bit_rate: f64,
    pub link_quality: i64,
    pub signal_level: i64,
    pub noise_level: i64,
}

#[allow(missing_docs)]
#[derive(Debug, Default, Clone, SerializeDict, DeserializeDict, Type)]
#[zvariant(signature = "a{sv}")]
pub struct Temperature {
    pub cpu: f64,
    pub gpu: f64,
    pub front_unit: f64,
    pub front_pcb: f64,
    pub backup_battery: f64,
    pub battery_pcb: f64,
    pub battery_cell: f64,
    pub liquid_lens: f64,
    pub main_accelerometer: f64,
    pub main_mcu: f64,
    pub mainboard: f64,
    pub security_accelerometer: f64,
    pub security_mcu: f64,
    pub battery_pack: f64,
    pub ssd: f64,
    pub wifi: f64,
    pub main_board_usb_hub_bot: f64,
    pub main_board_usb_hub_top: f64,
    pub main_board_security_supply: f64,
    pub main_board_audio_amplifier: f64,
    pub power_board_super_cap_charger: f64,
    pub power_board_pvcc_supply: f64,
    pub power_board_super_caps_left: f64,
    pub power_board_super_caps_right: f64,
    pub front_unit_850_730_left_top: f64,
    pub front_unit_850_730_left_bottom: f64,
    pub front_unit_850_730_right_top: f64,
    pub front_unit_850_730_right_bottom: f64,
    pub front_unit_940_left_top: f64,
    pub front_unit_940_left_bottom: f64,
    pub front_unit_940_right_top: f64,
    pub front_unit_940_right_bottom: f64,
    pub front_unit_940_center_top: f64,
    pub front_unit_940_center_bottom: f64,
    pub front_unit_white_top: f64,
    pub front_unit_shroud_rgb_top: f64,
}

#[allow(missing_docs)]
#[derive(Debug, Default, Clone, SerializeDict, DeserializeDict, Type)]
#[zvariant(signature = "a{sv}")]
pub struct Location {
    pub latitude: f64,
    pub longitude: f64,
}

#[allow(missing_docs)]
#[derive(Debug, Default, Clone, SerializeDict, DeserializeDict, Type)]
#[zvariant(signature = "a{sv}")]
pub struct Ssd {
    pub file_left: i64,
    pub space_left: i64,
    pub signup_left_to_upload: i64,
}

#[allow(missing_docs)]
#[derive(Debug, Default, Clone, SerializeDict, DeserializeDict, Type)]
#[zvariant(signature = "a{sv}")]
pub struct OrbVersion {
    pub current_release: String,
}
