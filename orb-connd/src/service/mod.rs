use crate::network_manager::{NetworkManager, WifiProfile, WifiSec};
use crate::secure_storage::SecureStorage;
use crate::utils::{IntoZResult, State};
use crate::OrbCapabilities;
use chrono::{DateTime, Utc};
use color_eyre::{
    eyre::{bail, eyre, Context},
    Result,
};
use orb_connd_dbus::{Connd, OBJ_PATH, SERVICE};
use orb_info::orb_os_release::OrbRelease;
use std::cmp;
use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;
use tokio::fs::{self, File};
use tokio::io::{self};
use tokio::task::{self, JoinHandle};
use tracing::{error, info, warn};
use wifi::Auth;
use wpa_conf::LegacyWpaConfig;

mod dbus;
mod mecard;
mod netconfig;
mod wifi;
mod wpa_conf;

pub struct ConndService {
    session_dbus: zbus::Connection,
    nm: NetworkManager,
    release: OrbRelease,
    cap: OrbCapabilities,
    magic_qr_applied_at: State<DateTime<Utc>>,
    connect_timeout: Duration,
    profile_storage: ProfileStorage,
}

#[derive(Debug)]
pub enum ProfileStorage {
    SecureStorage(SecureStorage),
    NetworkManager,
}

impl ProfileStorage {
    pub fn should_persist(&self) -> bool {
        matches!(self, Self::NetworkManager)
    }
}

impl ConndService {
    const NM_FOLDER: &str = "network-manager";
    const DEFAULT_CELLULAR_PROFILE: &str = "cellular";
    const DEFAULT_CELLULAR_APN: &str = "em";
    const DEFAULT_CELLULAR_IFACE: &str = "cdc-wdm0";
    const DEFAULT_WIFI_SSID: &str = "hotspot";
    const DEFAULT_WIFI_PSK: &str = "easytotypehardtoguess";
    const DEFAULT_WIFI_IFACE: &str = "wlan0";
    const MAGIC_QR_TIMESPAN_MIN: i64 = 10;
    const NM_STATE_MAX_SIZE_KB: u64 = 1024;
    const SECURE_STORAGE_KEY: &str = "nmprofiles";

    pub async fn new(
        session_dbus: zbus::Connection,
        nm: NetworkManager,
        release: OrbRelease,
        cap: OrbCapabilities,
        connect_timeout: Duration,
        usr_persistent: impl AsRef<Path>,
        profile_storage: ProfileStorage,
    ) -> Result<Self> {
        let usr_persistent = usr_persistent.as_ref();

        let connd = Self {
            session_dbus,
            nm,
            release,
            cap,
            magic_qr_applied_at: State::new(DateTime::default()),
            connect_timeout,
            profile_storage,
        };

        connd.setup_default_profiles().await?;

        let _ = async {
            connd.import_stored_profiles().await?;
            connd.import_legacy_wpa_conf(&usr_persistent).await?;
            connd.ensure_networking_enabled().await?;
            connd.ensure_nm_state_below_max_size(usr_persistent).await?;
            connd.commit_profiles_to_storage().await?;

            Ok::<_, color_eyre::Report>(())
        }
        .await
        .inspect_err(|e| {
            error!(error = ?e, "connd had a non-fatal startup error: {e}");
        });

        Ok(connd)
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

    async fn wifi_profile_add(
        &self,
        ssid: &str,
        sec: WifiSec,
        pwd: &str,
        hidden: bool,
    ) -> Result<()> {
        if ssid == Self::DEFAULT_CELLULAR_PROFILE || ssid == Self::DEFAULT_WIFI_SSID {
            bail!("{ssid} is not an allowed SSID name");
        }

        let already_saved = self
            .nm
            .list_wifi_profiles()
            .await
            .into_z()?
            .into_iter()
            .any(|profile| {
                profile.ssid == ssid
                    && profile.sec == sec
                    && profile.psk == pwd
                    && profile.hidden == hidden
            });

        if already_saved {
            info!("profile for ssid: {ssid}, already saved, exiting early");

            return Ok(());
        }

        let prio = self.get_next_priority().await?;

        self.nm
            .wifi_profile(ssid)
            .ssid(ssid)
            .sec(sec)
            .psk(pwd)
            .autoconnect(true)
            .priority(prio)
            .hidden(hidden)
            .persist(self.profile_storage.should_persist())
            .add()
            .await?;

        Ok(())
    }

    async fn ensure_networking_enabled(&self) -> Result<()> {
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

    async fn setup_default_profiles(&self) -> Result<()> {
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
                .sec(WifiSec::Wpa2Psk)
                .autoconnect(true)
                .hidden(false)
                .priority(-998)
                .persist(self.profile_storage.should_persist())
                .add()
                .await?;
        }

        Ok(())
    }

    async fn import_stored_profiles(&self) -> Result<()> {
        let ProfileStorage::SecureStorage(ss) = &self.profile_storage else {
            return Ok(());
        };

        let ss_profiles = ss
            .get(Self::SECURE_STORAGE_KEY.into())
            .await
            .wrap_err("failed trying to import from secure storage")?;

        let ss_profiles: Vec<WifiProfile> = if ss_profiles.is_empty() {
            Vec::new()
        } else {
            ciborium::de::from_reader(ss_profiles.as_slice())?
        };

        let nm_profiles = self.nm.list_wifi_profiles().await?;
        let nm_ssids: HashSet<_> = nm_profiles.iter().map(|p| &p.ssid).collect();

        let to_import = ss_profiles
            .into_iter()
            .filter(|p| !nm_ssids.contains(&p.ssid));

        for profile in to_import {
            self.wifi_profile_add(
                &profile.ssid,
                profile.sec,
                &profile.psk,
                profile.hidden,
            )
            .await?;
        }

        Ok(())
    }

    async fn commit_profiles_to_storage(&self) -> Result<()> {
        let ProfileStorage::SecureStorage(ss) = &self.profile_storage else {
            return Ok(());
        };

        let profiles = self.nm.list_wifi_profiles().await?;

        let mut bytes = Vec::new();
        ciborium::ser::into_writer(&profiles, &mut bytes)?;

        ss.put(Self::SECURE_STORAGE_KEY.into(), bytes)
            .await
            .wrap_err("failed trying to commit to secure storage")?;

        Ok(())
    }

    pub async fn import_legacy_wpa_conf(
        &self,
        wpa_conf_dir: impl AsRef<Path>,
    ) -> Result<()> {
        let wpa_conf_path = wpa_conf_dir.as_ref().join("wpa_supplicant-wlan0.conf");
        match File::open(&wpa_conf_path).await {
            Ok(file) => {
                let wpa_conf = LegacyWpaConfig::from_file(file).await?;

                if wpa_conf.ssid != Self::DEFAULT_WIFI_SSID {
                    self.wifi_profile_add(
                        &wpa_conf.ssid,
                        WifiSec::Wpa2Psk,
                        &wpa_conf.psk,
                        false,
                    )
                    .await?;
                } else {
                    info!("saved wpa config is default profile, no need to import it.")
                }

                fs::remove_file(wpa_conf_path).await?;
            }

            Err(e) if e.kind() == io::ErrorKind::NotFound => (),

            Err(e) => {
                return Err(e.into());
            }
        };

        Ok(())
    }

    /// returns true if anything was deleted because state was too big
    pub async fn ensure_nm_state_below_max_size(
        &self,
        usr_persistent: impl AsRef<Path>,
    ) -> Result<bool> {
        let nm_dir = usr_persistent.as_ref().join(Self::NM_FOLDER);
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

        let get_state_size = async || -> Result<u64> {
            let dir_size = dir_size_kb().await?;
            let ss_size = match &self.profile_storage {
            ProfileStorage::NetworkManager => 0,
            ProfileStorage::SecureStorage(ss) => ss
                .get(Self::SECURE_STORAGE_KEY.to_owned())
                .await
                .inspect_err(|e| error!("failed to read from secure storage when trying to calculate size: {e}"))
                .map(|bytes| bytes.len())
                .unwrap_or_default() as u64,
        };

            Ok(dir_size + ss_size)
        };

        let state_size = get_state_size().await?;
        if state_size < Self::NM_STATE_MAX_SIZE_KB {
            info!("{nm_dir:?} plus SecureStorage-{} is below 1024kB. current size {state_size}kB", Self::SECURE_STORAGE_KEY);
            return Ok(false);
        }

        warn!("{nm_dir:?} plus SecureStorage-{} is above 1024kB. current size {state_size}kB. attempting to reduce size", Self::SECURE_STORAGE_KEY);

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

        self.commit_profiles_to_storage().await?;

        // remove dhcp leases and seen-bssids
        let varlib = usr_persistent.as_ref().join(Self::NM_FOLDER).join("varlib");

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

        let dir_size = get_state_size().await?;
        if dir_size < Self::NM_STATE_MAX_SIZE_KB {
            info!("successfully reduced nm state size to {dir_size}kB");
            Ok(true)
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

impl TryInto<WifiSec> for Auth {
    type Error = color_eyre::Report;

    fn try_into(self) -> std::result::Result<WifiSec, Self::Error> {
        let sec = match self {
            Auth::Sae => WifiSec::Wpa3Sae,
            Auth::Wpa => WifiSec::Wpa2Psk,
            _ => bail!("{self:?} is not supported"),
        };

        Ok(sec)
    }
}

impl TryFrom<WifiSec> for Auth {
    type Error = color_eyre::Report;

    fn try_from(value: WifiSec) -> Result<Auth> {
        use WifiSec::*;
        let auth = match value {
            Wep => Auth::Wep,
            Wpa2Psk => Auth::Wpa,
            Wpa3Sae => Auth::Sae,
            Open => Auth::Nopass,
            _ => bail!("{value} cannot be converted back to Auth enum"),
        };

        Ok(auth)
    }
}
