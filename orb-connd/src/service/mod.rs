use std::cmp;

use crate::network_manager::NetworkManager;
use crate::network_manager::WifiSec;
use crate::qr;
use crate::utils::IntoZResult;
use async_trait::async_trait;
use color_eyre::Result;
use orb_connd_dbus::{Connd, ConndT, OBJ_PATH, SERVICE};
use orb_info::orb_os_release::OrbRelease;
use tokio::task;
use tokio::task::JoinHandle;
use tracing::error;
use zbus::fdo::Error as ZErr;
use zbus::fdo::Result as ZResult;

mod wifi;

pub struct ConndService {
    conn: zbus::Connection,
    nm: NetworkManager,
    release: OrbRelease,
}

impl ConndService {
    const CELLULAR_PROFILE: &str = "cellular";

    pub fn new(conn: zbus::Connection, release: OrbRelease) -> Self {
        Self {
            nm: NetworkManager::new(conn.clone()),
            conn,
            release,
        }
    }

    pub fn spawn(self) -> JoinHandle<Result<()>> {
        let conn = self.conn.clone();

        task::spawn(async move {
            conn.request_name(SERVICE)
                .await
                .inspect_err(|e| error!("failed to request name on dbus {e}"))?;

            conn.object_server()
                .at(OBJ_PATH, Connd::from(self))
                .await
                .inspect_err(|e| error!("failed to serve obj on dbus {e}"))?;

            futures::future::pending::<()>().await;

            Ok(())
        })
    }

    /// increments priority of newly added networks up to 999
    /// so the last added network is always higher priority than others
    async fn get_next_priority(&self) -> Result<i32> {
        let lowest_prio = self
            .nm
            .list_wifi_profiles()
            .await?
            .into_iter()
            .map(|profile| profile.priority)
            .min()
            .unwrap_or(-1000);

        let prio = cmp::min(lowest_prio + 1, 999);

        Ok(prio)
    }
}

#[async_trait]
impl ConndT for ConndService {
    async fn create_softap(&self, _ssid: String, _pwd: String) -> ZResult<()> {
        Err(e("not yet implemented!"))
    }

    async fn remove_softap(&self, _ssid: String) -> ZResult<()> {
        Err(e("not yet implemented!"))
    }

    async fn add_wifi_profile(
        &self,
        ssid: String,
        sec: String,
        pwd: String,
    ) -> ZResult<()> {
        if ssid == Self::CELLULAR_PROFILE {
            return Err(e(&format!(
                "{} is not an allowed SSID name",
                Self::CELLULAR_PROFILE
            )));
        }

        let Some(sec) = WifiSec::from_str(&sec) else {
            return Err(e("invalid sec"));
        };

        let prio = self.get_next_priority().await.into_z()?;

        self.nm
            .wifi_profile(&ssid)
            .ssid(&ssid)
            .sec(sec)
            .pwd(&pwd)
            .autoconnect(true)
            .priority(prio)
            .add()
            .await
            .into_z()?;

        Ok(())
    }

    async fn remove_wifi_profile(&self, ssid: String) -> ZResult<()> {
        if ssid == Self::CELLULAR_PROFILE {
            return Err(e(&format!(
                "{} is not an allowed SSID name",
                Self::CELLULAR_PROFILE
            )));
        }

        self.nm.remove_profile(&ssid).await.into_z()?;

        Ok(())
    }

    async fn apply_wifi_qr(&self, _contents: String) -> ZResult<()> {
        println!("AM I HERE????");
        Err(e("not yet implemented!"))
    }

    async fn apply_netconfig_qr(
        &self,
        contents: String,
        check_ts: bool,
    ) -> ZResult<()> {
        qr::verify(&contents, self.release).into_z()?;
        Ok(())
    }
}

fn e(str: &str) -> ZErr {
    ZErr::Failed(str.to_string())
}
