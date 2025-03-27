use eyre::Result;
use orb_backend_status_dbus::{BackendStatusProxy, WifiNetwork};
use zbus::Connection;

use crate::data::NetworkInfo;

pub struct BackendStatus {
    backend_status_proxy: BackendStatusProxy<'static>,
}

impl BackendStatus {
    pub async fn new(connection: &Connection) -> Result<Self> {
        let backend_status_proxy = BackendStatusProxy::new(connection).await?;
        Ok(Self {
            backend_status_proxy,
        })
    }

    pub async fn send_location_data(&self, network_info: &NetworkInfo) -> Result<()> {
        let dbus_wifi_networks = network_info
            .wifi
            .iter()
            .map(|wifi| WifiNetwork {
                bssid: wifi.bssid.clone(),
                frequency: wifi.frequency,
                signal_level: wifi.signal_level,
                flags: wifi.flags.clone(),
                ssid: wifi.ssid.clone(),
            })
            .collect();

        self.backend_status_proxy
            .provide_wifi_networks(dbus_wifi_networks)
            .await?;
        Ok(())
    }
}
