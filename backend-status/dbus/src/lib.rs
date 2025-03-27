//! Receives status updates from the orb for the backend service

use serde::{Deserialize, Serialize};
use zbus::{
    interface,
    zvariant::{OwnedValue, Type, Value},
};

pub trait BackendStatusIface: Send + Sync + 'static {
    fn provide_wifi_networks(&mut self, wifi_networks: Vec<WifiNetwork>);
}

#[derive(Debug, derive_more::From)]
pub struct BackendStatus<T>(pub T);

#[interface(
    name = "org.worldcoin.BackendStatus1",
    proxy(
        default_service = "org.worldcoin.BackendStatus1",
        default_path = "/org/worldcoin/BackendStatus1",
    )
)]
impl<T: BackendStatusIface> BackendStatusIface for BackendStatus<T> {
    fn provide_wifi_networks(&mut self, wifi_networks: Vec<WifiNetwork>) {
        self.0.provide_wifi_networks(wifi_networks)
    }
}

#[derive(
    Debug, Serialize, Deserialize, Type, Clone, Eq, PartialEq, Value, OwnedValue,
)]
pub struct WifiNetwork {
    pub bssid: String,
    pub frequency: u32,
    pub signal_level: i32,
    pub flags: String,
    pub ssid: String,
}
