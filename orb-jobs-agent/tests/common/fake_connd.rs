use async_trait::async_trait;
use mockall::mock;
use orb_connd_dbus::{ConndT, ConnectionState, NetConfig};
use zbus::fdo::Result;

mock! {
    pub Connd {}

    #[async_trait]
    impl ConndT for Connd {
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
}
