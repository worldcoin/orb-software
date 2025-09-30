use crate::network_manager::{NetworkManager, WifiSec};
use crate::utils::{IntoZResult, State};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use color_eyre::{eyre::ContextCompat, Result};
use netconfig::NetConfig;
use orb_connd_dbus::{Connd, ConndT, OBJ_PATH, SERVICE};
use orb_info::orb_os_release::{OrbOsPlatform, OrbRelease};
use std::cmp;
use std::collections::HashMap;
use std::path::Path;
use tokio::fs::{self, File};
use tokio::io::{self, AsyncReadExt};
use tokio::task::{self, JoinHandle};
use tracing::{error, info, warn};
use zbus::fdo::{Error as ZErr, Result as ZResult};

mod netconfig;
mod wifi;

pub struct ConndService {
    session_dbus: zbus::Connection,
    nm: NetworkManager,
    release: OrbRelease,
    platform: OrbOsPlatform,
    magic_qr_applied_at: State<DateTime<Utc>>,
}

impl ConndService {
    const DEFAULT_CELLULAR_PROFILE: &str = "cellular";
    const DEFAULT_CELLULAR_APN: &str = "em";
    const DEFAULT_CELLULAR_IFACE: &str = "cdc-wdm0";
    const DEFAULT_WIFI_SSID: &str = "hotspot";
    const DEFAULT_WIFI_PSK: &str = "easytotypehardtoguess";
    const DEFAULT_WIFI_IFACE: &str = "wlan0";
    const MAGIC_QR_TIMESPAN_MIN: i64 = 10;

    pub fn new(
        session_dbus: zbus::Connection,
        system_dbus: zbus::Connection,
        release: OrbRelease,
        platform: OrbOsPlatform,
    ) -> Self {
        Self {
            session_dbus,
            nm: NetworkManager::new(system_dbus),
            release,
            platform,
            magic_qr_applied_at: State::new(DateTime::default()),
        }
    }

    pub fn spawn(self) -> JoinHandle<Result<()>> {
        info!("spawning dbus service {SERVICE} at path {OBJ_PATH}!");
        let conn = self.session_dbus.clone();

        task::spawn(async move {
            conn.request_name(SERVICE)
                .await
                .inspect_err(|e| error!("failed to request name on dbus {e}"))?;

            conn.object_server()
                .at(OBJ_PATH, Connd::from(self))
                .await
                .inspect_err(|e| error!("failed to serve obj on dbus {e}"))?;

            info!("dbus service spawned successfully!");
            futures::future::pending::<()>().await;

            Ok(())
        })
    }

    pub async fn setup_default_profiles(&self) -> Result<()> {
        let cel_profiles = self.nm.list_cellular_profiles().await?;
        let default_cel_profile_exists = cel_profiles
            .iter()
            .any(|p| p.id == Self::DEFAULT_CELLULAR_PROFILE);

        if !default_cel_profile_exists {
            self.nm
                .cellular_profile(Self::DEFAULT_CELLULAR_PROFILE)
                .apn(Self::DEFAULT_CELLULAR_APN)
                .iface(Self::DEFAULT_CELLULAR_IFACE)
                .priority(-999)
                .add()
                .await?;
        }

        let wifi_profiles = self.nm.list_wifi_profiles().await?;
        let default_wifi_profile_exists = wifi_profiles.iter().any(|p| {
            p.ssid == Self::DEFAULT_WIFI_SSID && p.psk == Self::DEFAULT_WIFI_PSK
        });

        if !default_wifi_profile_exists {
            self.nm
                .wifi_profile(Self::DEFAULT_WIFI_SSID)
                .ssid(Self::DEFAULT_WIFI_SSID)
                .psk(Self::DEFAULT_WIFI_PSK)
                .sec(WifiSec::WpaPsk)
                .autoconnect(true)
                .hidden(false)
                .priority(-998)
                .add()
                .await?;
        }

        Ok(())
    }

    pub async fn import_wpa_conf(&self, wpa_conf_dir: impl AsRef<Path>) -> Result<()> {
        let wpa_conf = wpa_conf_dir.as_ref().join("wpa_supplicant-wlan0.conf");
        match File::open(&wpa_conf).await {
            Ok(mut file) => {
                let mut contents = String::new();
                file.read_to_string(&mut contents).await?;

                let map: HashMap<_, _> = contents
                    .lines()
                    .filter_map(|line| line.trim().split_once("="))
                    .collect();

                let ssid = map
                    .get("ssid")
                    .wrap_err("could not parse ssid")?
                    .trim_matches('"');

                let psk = map.get("psk").wrap_err("could not parse psk")?;

                self.add_wifi_profile(
                    ssid.to_string(),
                    "wpa".into(),
                    psk.to_string(),
                    false,
                )
                .await?;

                fs::remove_file(wpa_conf).await?;
            }

            Err(e) if e.kind() == io::ErrorKind::NotFound => (),

            Err(e) => {
                return Err(e.into());
            }
        };

        Ok(())
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
            .max()
            .unwrap_or(-1000);

        let prio = cmp::min(lowest_prio + 1, 999);

        Ok(prio)
    }
}

#[async_trait]
impl ConndT for ConndService {
    async fn create_softap(&self, ssid: String, _pwd: String) -> ZResult<()> {
        info!("received request to create softap with ssid {ssid}");
        Err(e("not yet implemented!"))
    }

    async fn remove_softap(&self, ssid: String) -> ZResult<()> {
        info!("received request to remove softap with ssid {ssid}");
        Err(e("not yet implemented!"))
    }

    async fn add_wifi_profile(
        &self,
        ssid: String,
        sec: String,
        pwd: String,
        hidden: bool,
    ) -> ZResult<()> {
        info!("trying to add wifi profile with ssid {ssid}");
        if ssid == Self::DEFAULT_CELLULAR_PROFILE {
            return Err(e(&format!(
                "{} is not an allowed SSID name",
                Self::DEFAULT_CELLULAR_PROFILE
            )));
        }

        let Some(sec) = WifiSec::parse(&sec) else {
            return Err(e("invalid sec"));
        };

        let prio = self.get_next_priority().await.into_z()?;

        self.nm
            .wifi_profile(&ssid)
            .ssid(&ssid)
            .sec(sec)
            .psk(&pwd)
            .autoconnect(true)
            .priority(prio)
            .hidden(hidden)
            .add()
            .await
            .into_z()?;

        Ok(())
    }

    async fn remove_wifi_profile(&self, ssid: String) -> ZResult<()> {
        info!("trying to remove wifi profile with ssid {ssid}");
        if ssid == Self::DEFAULT_CELLULAR_PROFILE {
            return Err(e(&format!(
                "{} is not an allowed SSID name",
                Self::DEFAULT_CELLULAR_PROFILE
            )));
        }

        self.nm.remove_profile(&ssid).await.into_z()?;

        Ok(())
    }

    async fn apply_wifi_qr(&self, contents: String) -> ZResult<()> {
        info!("trying to apply wifi qr code {contents}");
        let skip_wifi_qr_restrictions = self.release == OrbRelease::Dev;

        if !skip_wifi_qr_restrictions {
            let has_no_connectivity = !self.nm.has_connectivity().await.into_z()?;
            let magic_qr_applied_at = self
                .magic_qr_applied_at
                .read(|x| *x)
                .map_err(|_| e("magic qr mtx err"))?;

            let within_magic_qr_timespan = (Utc::now() - magic_qr_applied_at)
                .num_minutes()
                < Self::MAGIC_QR_TIMESPAN_MIN;

            let can_apply_wifi_qr = has_no_connectivity || within_magic_qr_timespan;

            if !can_apply_wifi_qr {
                return Err(e(
                    "we already have internet connectivity, use signed qr instead",
                ));
            }
        }

        let creds = wifi::Credentials::parse(&contents).into_z()?;

        let saved_profile = self
            .nm
            .list_wifi_profiles()
            .await
            .into_z()?
            .into_iter()
            .find(|p| p.ssid == creds.ssid);

        match (saved_profile, creds.psk) {
            // profile exists and no pwd was provided
            (Some(profile), None) => {
                self.nm
                    .connect_to_wifi(&profile.path, Self::DEFAULT_WIFI_IFACE)
                    .await
                    .into_z()?;
            }

            // pwd was provided so we assume a new profile is being added
            (Some(_), Some(psk)) | (None, Some(psk)) => {
                self.add_wifi_profile(
                    creds.ssid,
                    creds.sec.as_str().into(), // TODO: dont parse twice lmao
                    psk,
                    creds.hidden,
                )
                .await?;
            }

            // no pwd provided and no existing profile, nothing we can do
            (None, None) => {
                return Err(e(&format!(
                    "wifi profile '{}' does not exist and no password was provided",
                    creds.ssid,
                )))
            }
        }

        info!("applied wifi qr successfully");

        Ok(())
    }

    async fn apply_netconfig_qr(
        &self,
        contents: String,
        check_ts: bool,
    ) -> ZResult<()> {
        async {
            info!("trying to apply netconfig qr code {contents}");
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
                let saved_profile = self
                    .nm
                    .list_wifi_profiles()
                    .await
                    .into_z()?
                    .into_iter()
                    .find(|p| p.ssid == wifi_creds.ssid);

                match (saved_profile, wifi_creds.psk) {
                    // profile exists and no pwd was provided
                    (Some(profile), None) => {
                        self.nm
                            .connect_to_wifi(&profile.path, Self::DEFAULT_WIFI_IFACE)
                            .await
                            .into_z()?;
                    }

                    // pwd was provided so we assume a new profile is being added
                    (Some(_), Some(psk)) | (None, Some(psk)) => {
                        self.add_wifi_profile(
                            wifi_creds.ssid,
                            wifi_creds.sec.as_str().into(), // TODO: dont parse twice lmao
                            psk,
                            wifi_creds.hidden,
                        )
                        .await?;
                    }

                    // no pwd provided and no existing profile, nothing we can do
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

            info!("applied netconfig qr successfully");

            Ok(())
        }
        .await
        .inspect_err(|e| error!("failed to apply netconfig qr with {e}"))
    }

    async fn apply_magic_reset_qr(&self) -> ZResult<()> {
        info!("trying to apply magic reset qr");
        self.nm.set_wifi(false).await.into_z()?;

        let wifi_profiles = self.nm.list_wifi_profiles().await.into_z()?;
        for profile in wifi_profiles {
            if profile.ssid == Self::DEFAULT_WIFI_SSID {
                continue;
            }

            self.nm.remove_profile(&profile.id).await.into_z()?;
        }

        self.nm.set_wifi(true).await.into_z()?;
        self.magic_qr_applied_at
            .write(|val| *val = Utc::now())
            .map_err(|_| e("magic qr mtx err"))?;

        info!("successfuly applied magic reset qr");

        Ok(())
    }

    async fn has_connectivity(&self) -> ZResult<bool> {
        self.nm.has_connectivity().await.into_z()
    }
}

fn e(str: &str) -> ZErr {
    ZErr::Failed(str.to_string())
}
