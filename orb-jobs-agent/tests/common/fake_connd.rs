use async_trait::async_trait;
use mockall::mock;
use orb_connd_dbus::ConndT;
use zbus::fdo::Result;

mock! {
    pub Connd {}

    #[async_trait]
    impl ConndT for Connd {
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
        async fn apply_wifi_qr(&self, contents: String) -> Result<()>;
        async fn apply_netconfig_qr(&self, contents: String, check_ts: bool) -> Result<()>;
        async fn apply_magic_reset_qr(&self) -> Result<()>;
        async fn has_connectivity(&self) -> Result<bool>;
    }
}
