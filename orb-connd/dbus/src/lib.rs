use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use zbus::fdo::Result;
use zbus::interface;
use zbus::zvariant::Type;

pub const SERVICE: &str = "org.worldcoin.Connd";
pub const IFACE: &str = "org.worldcoin.Connd1";
pub const OBJ_PATH: &str = "/org/worldcoin/Connd1";

#[async_trait]
pub trait ConndT: 'static + Send + Sync {
    async fn create_softap(&self, ssid: String, pwd: String) -> Result<()>;
    async fn remove_softap(&self, ssid: String) -> Result<()>;
    async fn add_wifi_profile(
        &self,
        ssid: String,
        sec: String,
        pwd: String,
        hidden: bool,
    ) -> Result<()>;
    async fn remove_wifi_profile(&self, ssid: String) -> Result<()>;
    async fn connect_to_wifi(&self, ssid: String) -> Result<()>;
    async fn list_wifi_profiles(&self) -> Result<Vec<WifiProfile>>;
    async fn scan_wifi(&self) -> Result<Vec<AccessPoint>>;
    async fn netconfig_set(
        &self,
        wifi: bool,
        smart_switching: bool,
        airplane_mode: bool,
    ) -> Result<NetConfig>;
    async fn netconfig_get(&self) -> Result<NetConfig>;
    async fn apply_wifi_qr(&self, contents: String) -> Result<()>;
    async fn apply_netconfig_qr(&self, contents: String, check_ts: bool) -> Result<()>;
    async fn apply_magic_reset_qr(&self) -> Result<()>;
    async fn connection_state(&self) -> Result<ConnectionState>;
}

#[derive(Debug, derive_more::From)]
pub struct Connd<T>(pub T);

#[interface(
    name = "org.worldcoin.Connd1",
    proxy(
        default_service = "org.worldcoin.Connd",
        default_path = "/org/worldcoin/Connd1",
    )
)]
#[async_trait]
impl<T: ConndT> ConndT for Connd<T> {
    async fn create_softap(&self, ssid: String, pwd: String) -> Result<()> {
        self.0.create_softap(ssid, pwd).await
    }

    async fn remove_softap(&self, ssid: String) -> Result<()> {
        self.0.remove_softap(ssid).await
    }

    async fn add_wifi_profile(
        &self,
        ssid: String,
        sec: String,
        pwd: String,
        hidden: bool,
    ) -> Result<()> {
        self.0.add_wifi_profile(ssid, sec, pwd, hidden).await
    }

    async fn remove_wifi_profile(&self, ssid: String) -> Result<()> {
        self.0.remove_wifi_profile(ssid).await
    }

    async fn connect_to_wifi(&self, ssid: String) -> Result<()> {
        self.0.connect_to_wifi(ssid).await
    }

    async fn list_wifi_profiles(&self) -> Result<Vec<WifiProfile>> {
        self.0.list_wifi_profiles().await
    }

    async fn scan_wifi(&self) -> Result<Vec<AccessPoint>> {
        self.0.scan_wifi().await
    }

    async fn netconfig_set(
        &self,
        wifi: bool,
        smart_switching: bool,
        airplane_mode: bool,
    ) -> Result<NetConfig> {
        self.0
            .netconfig_set(wifi, smart_switching, airplane_mode)
            .await
    }

    async fn netconfig_get(&self) -> Result<NetConfig> {
        self.0.netconfig_get().await
    }

    async fn apply_wifi_qr(&self, contents: String) -> Result<()> {
        self.0.apply_wifi_qr(contents).await
    }

    async fn apply_netconfig_qr(&self, contents: String, check_ts: bool) -> Result<()> {
        self.0.apply_netconfig_qr(contents, check_ts).await
    }

    async fn apply_magic_reset_qr(&self) -> Result<()> {
        self.0.apply_magic_reset_qr().await
    }

    async fn connection_state(&self) -> Result<ConnectionState> {
        self.0.connection_state().await
    }
}

#[derive(Debug, Clone, Type, Serialize, Deserialize, PartialEq)]
pub struct WifiProfile {
    pub ssid: String,
    pub sec: String,
    pub psk: String,
}

#[derive(Debug, Clone, Type, PartialEq, Deserialize, Serialize)]
pub struct NetConfig {
    pub wifi: bool,
    pub smart_switching: bool,
    pub airplane_mode: bool,
}

#[derive(Debug, Clone, PartialEq, Type, Serialize, Deserialize)]
pub struct AccessPoint {
    pub ssid: String,
    pub bssid: String,
    pub is_saved: bool,
    pub freq_mhz: u32,
    pub max_bitrate_kbps: u32,
    pub strength_pct: u8,
    pub last_seen: String,
    pub mode: String,
    pub capabilities: AccessPointCapabilities,
    pub sec: String,
}

#[derive(Debug, Clone, PartialEq, Type, Serialize, Deserialize, Default)]
pub struct AccessPointCapabilities {
    /// WEP/WPA/WPA2/3 required (not "open")
    pub privacy: bool,
    /// WPS supported
    pub wps: bool,
    /// WPS push-button
    pub wps_pbc: bool,
    /// WPS PIN
    pub wps_pin: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Type, Serialize, Deserialize)]
pub enum ConnectionState {
    Disconnected,
    Disconnecting,
    Connecting,
    /// There is IPv4 and/or IPv6 connectivity, but not global. We are connected to the network but
    /// there is no internet connectivity.
    PartiallyConnected,
    Connected,
}
