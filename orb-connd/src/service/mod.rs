use crate::network_manager::NetworkManager;
use crate::network_manager::WifiSec;
use crate::utils::IntoZResult;
use async_trait::async_trait;
use chrono::Utc;
use color_eyre::Result;
use netconfig::NetConfig;
use orb_connd_dbus::{Connd, ConndT, OBJ_PATH, SERVICE};
use orb_info::orb_os_release::OrbOsPlatform;
use orb_info::orb_os_release::OrbRelease;
use std::cmp;
use tokio::task;
use tokio::task::JoinHandle;
use tracing::error;
use tracing::warn;
use zbus::fdo::Error as ZErr;
use zbus::fdo::Result as ZResult;

mod netconfig;
mod wifi;

pub struct ConndService {
    conn: zbus::Connection,
    nm: NetworkManager,
    release: OrbRelease,
    platform: OrbOsPlatform,
}

impl ConndService {
    const CELLULAR_PROFILE: &str = "cellular";

    pub fn new(
        conn: zbus::Connection,
        release: OrbRelease,
        platform: OrbOsPlatform,
    ) -> Self {
        Self {
            nm: NetworkManager::new(conn.clone()),
            conn,
            release,
            platform,
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
        NetConfig::verify_signature(&contents, self.release).into_z()?;
        let netconf = NetConfig::parse(&contents).into_z()?;

        if check_ts {
            let now = Utc::now();
            let delta = now - netconf.created_at;
            if delta.num_minutes() > 10 {
                return Err(e("qr code was created more than 10min ago"));
            }
        }

        if let Some(wifi_creds) = netconf.wifi_credentials {
            let exists = self
                .nm
                .list_wifi_profiles()
                .await
                .into_z()?
                .into_iter()
                .find(|p| p.ssid == wifi_creds.ssid);

            match (exists, wifi_creds.psk) {
                (Some(profile), _) => {
                    self.nm.connect_to_wifi(&profile).await.into_z()?;
                }

                (None, Some(psk)) => {
                    self.add_wifi_profile(
                        wifi_creds.ssid,
                        wifi_creds.sec.as_str().into(), // TODO: dont parse twice lmao
                        psk,
                    )
                    .await?;
                }

                (None, None) => {
                    return Err(e(&format!(
                        "wifi profile '{}' does not exist and no password was provided",
                        wifi_creds.ssid,
                    )))
                }
            }
        };

        // Pearl orbs do not support extra NetConfig fields
        if self.platform == OrbOsPlatform::Pearl {
            return Ok(());
        }

        if let Some(_airplane_mode) = netconf.airplane_mode {
            warn!("airplane mode is not supported yet!");
        }

        if let Some(wifi_enabled) = netconf.wifi_enabled {
            self.nm.set_wifi(wifi_enabled).await.into_z()?;
        }

        if let Some(airplane_mode) = netconf.airplane_mode {
            self.nm.set_smart_switching(airplane_mode).await.into_z()?;
        }

        Ok(())
    }
}

fn e(str: &str) -> ZErr {
    ZErr::Failed(str.to_string())
}
