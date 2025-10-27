use crate::network_manager::{NetworkManager, WifiSec};
use crate::utils::{IntoZResult, State};
use crate::OrbCapabilities;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use color_eyre::eyre::eyre;
use color_eyre::{
    eyre::{bail, ContextCompat},
    Result,
};
use netconfig::NetConfig;
use orb_connd_dbus::{Connd, ConndT, OBJ_PATH, SERVICE};
use orb_info::orb_os_release::OrbRelease;
use std::cmp;
use std::collections::HashMap;
use std::path::Path;
use tokio::fs::{self, File};
use tokio::io::{self, AsyncReadExt};
use tokio::task::{self, JoinHandle};
use tracing::{error, info, warn};
use wifi::Auth;
use zbus::fdo::{Error as ZErr, Result as ZResult};

mod mecard;
mod netconfig;
mod wifi;

pub struct ConndService {
    session_dbus: zbus::Connection,
    nm: NetworkManager,
    release: OrbRelease,
    cap: OrbCapabilities,
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
    const NM_STATE_MAX_SIZE_KB: u64 = 1024;

    pub fn new(
        session_dbus: zbus::Connection,
        system_dbus: zbus::Connection,
        release: OrbRelease,
        cap: OrbCapabilities,
    ) -> Self {
        Self {
            session_dbus,
            nm: NetworkManager::new(system_dbus),
            release,
            cap,
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

    pub async fn ensure_networking_enabled(&self) -> Result<()> {
        if !self.nm.networking_enabled().await? {
            self.nm.set_networking(true).await?;
        }

        if !self.nm.wwan_enabled().await? {
            self.nm.set_wwan(true).await?;
        }

        if self.release != OrbRelease::Dev
            && self.cap == OrbCapabilities::WifiOnly
            && !self.nm.wifi_enabled().await?
        {
            self.nm.set_wifi(true).await?;
        }

        Ok(())
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

    pub async fn ensure_nm_state_below_max_size(
        &self,
        usr_persistent: impl AsRef<Path>,
    ) -> Result<()> {
        let nm_dir = usr_persistent.as_ref().join("network-manager");
        let dir_size_kb = async || -> Result<u64> {
            let mut total_bytes = 0u64;
            let mut stack = vec![nm_dir.clone()];

            while let Some(dir) = stack.pop() {
                let mut dir = fs::read_dir(&dir).await?;

                while let Some(e) = dir.next_entry().await? {
                    let ft = e.file_type().await?;

                    if ft.is_file() {
                        total_bytes += e.metadata().await?.len();
                    } else if ft.is_dir() {
                        stack.push(e.path());
                    }
                }
            }

            Ok(total_bytes / 1024)
        };

        let dir_size = dir_size_kb().await?;
        if dir_size < Self::NM_STATE_MAX_SIZE_KB {
            info!("/usr/persistent/network-manager is below 1024kB. current size {dir_size}");
            return Ok(());
        }

        warn!("/usr/persistent/network-manager is above 1024kB. current size {dir_size}. attempting to reduce size");

        // remove excess wifi profiles
        let mut wifi_profiles = self.nm.list_wifi_profiles().await?;
        wifi_profiles.sort_by_key(|p| p.priority);

        let profiles_to_keep = 2;
        let profiles_to_remove = wifi_profiles.len().saturating_sub(profiles_to_keep);

        for profile in wifi_profiles.into_iter().take(profiles_to_remove) {
            if profile.id == Self::DEFAULT_WIFI_SSID {
                continue;
            }

            self.nm.remove_profile(&profile.id).await?;
        }

        // remove dhcp leases and seen-bssids
        let varlib = usr_persistent
            .as_ref()
            .join("network-manager")
            .join("varlib");

        let seen_bssids = varlib.join("seen-bssids");
        let mut to_delete = vec![seen_bssids];

        let mut varlib = fs::read_dir(&varlib).await?;
        while let Some(entry) = varlib.next_entry().await? {
            let ft = entry.file_type().await?;
            if !ft.is_file() {
                continue;
            }

            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "lease") {
                to_delete.push(path);
            }
        }

        for filepath in to_delete {
            fs::remove_file(filepath).await?;
        }

        let dir_size = dir_size_kb().await?;
        if dir_size < Self::NM_STATE_MAX_SIZE_KB {
            info!("successfully reduced nm state size to {dir_size}kB");
            Ok(())
        } else {
            Err(eyre!(
                "directory too big even after wiping files. size: {dir_size}kB"
            ))
        }
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

        let path = self
            .nm
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

        let aps = self
            .nm
            .wifi_scan()
            .await
            .inspect_err(|e| error!("failed to scan for wifi networks due to err {e}"))
            .into_z()?;

        for ap in aps {
            if ap == ssid {
                if let Err(e) = self
                    .nm
                    .connect_to_wifi(&path, Self::DEFAULT_WIFI_IFACE)
                    .await
                {
                    error!("failed to connect to wifi ssid {ssid} due to err {e}");
                }

                break;
            }
        }

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
        async {
            info!("trying to apply wifi qr code");
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
            let sec: WifiSec = creds.auth.try_into().into_z()?;
            info!(ssid = creds.ssid, "adding wifi network");

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
                        sec.as_str().to_string(),
                        psk.0,
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
        .await
        .inspect_err(|e| error!("failed to apply wifi qr with {e}"))
    }

    async fn apply_netconfig_qr(
        &self,
        contents: String,
        check_ts: bool,
    ) -> ZResult<()> {
        async {
            info!("trying to apply netconfig qr code");
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
                let sec: WifiSec = wifi_creds.auth.try_into().into_z()?;
                info!(ssid = wifi_creds.ssid, "adding wifi network from netconfig");

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
                            sec.as_str().to_string(),
                            psk.0,
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

            // Orbs without cellular do not support extra NetConfig fields
            if self.cap == OrbCapabilities::WifiOnly {
                return Ok(());
            }

            if let Some(_airplane_mode) = netconf.airplane_mode {
                warn!("airplane mode is not supported yet!");
            }

            if let Some(wifi_enabled) = netconf.wifi_enabled {
                self.nm.set_wifi(wifi_enabled).await.into_z()?;
            }

            if let Some(smart_switching) = netconf.smart_switching {
                self.nm
                    .set_smart_switching(smart_switching)
                    .await
                    .into_z()?;
            }

            info!(
                airplane_mode = netconf.airplane_mode,
                wifi_enabled = netconf.wifi_enabled,
                smart_switching = netconf.smart_switching,
                "applied netconfig qr successfully!"
            );

            Ok(())
        }
        .await
        .inspect_err(|e| error!("failed to apply netconfig qr with {e}"))
    }

    async fn apply_magic_reset_qr(&self) -> ZResult<()> {
        info!("trying to apply magic reset qr");

        let wifi_profiles = self.nm.list_wifi_profiles().await.into_z()?;
        for profile in wifi_profiles {
            if profile.ssid == Self::DEFAULT_WIFI_SSID {
                continue;
            }

            self.nm.remove_profile(&profile.id).await.into_z()?;
        }

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

impl TryInto<WifiSec> for Auth {
    type Error = color_eyre::Report;

    fn try_into(self) -> std::result::Result<WifiSec, Self::Error> {
        let sec = match self {
            Auth::Sae => WifiSec::Wpa3Sae,
            Auth::Wpa => WifiSec::WpaPsk,
            _ => bail!("{self:?} is not supported"),
        };

        Ok(sec)
    }
}
