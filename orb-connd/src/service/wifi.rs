use crate::network_manager::WifiSec;

#[derive(Debug, PartialEq, Clone)]
pub struct Credentials {
    pub ssid: String,
    pub sec: WifiSec,
    pub psk: Option<String>,
}
