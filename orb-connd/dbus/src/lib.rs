use async_trait::async_trait;
use zbus::fdo::Result;
use zbus::interface;

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
    ) -> Result<()>;
    async fn remove_wifi_profile(&self, ssid: String) -> Result<()>;
    async fn apply_wifi_qr(&self, contents: String) -> Result<()>;
    async fn apply_netconfig_qr(&self, contents: String) -> Result<()>;
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
    ) -> Result<()> {
        self.0.add_wifi_profile(ssid, sec, pwd).await
    }

    async fn remove_wifi_profile(&self, ssid: String) -> Result<()> {
        self.0.remove_wifi_profile(ssid).await
    }

    async fn apply_wifi_qr(&self, contents: String) -> Result<()> {
        self.0.apply_wifi_qr(contents).await
    }

    async fn apply_netconfig_qr(&self, contents: String) -> Result<()> {
        self.0.apply_netconfig_qr(contents).await
    }
}
