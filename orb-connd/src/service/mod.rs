use crate::network_manager::{
    AccessPoint, ActiveConnState, NetworkManager, WifiProfile, WifiSec,
};
use crate::profile_store::ProfileStore;
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
use orb_connd_dbus::{Connd, ConndT, ConnectionState, OBJ_PATH, SERVICE};
use orb_info::orb_os_release::OrbRelease;
use rusty_network_manager::dbus_interface_types::{
    NM80211Mode, NMConnectivityState, NMState,
};
use std::cmp;
use std::path::Path;
use std::time::Duration;
use tokio::fs::{self, File};
use tokio::io::{self};
use tokio::task::{self, JoinHandle};
use tracing::{error, info, warn};
use wifi::Auth;
use wpa_conf::LegacyWpaConfig;
use zbus::fdo::{Error as ZErr, Result as ZResult};

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
    profile_store: ProfileStore,
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

    pub async fn new(
        session_dbus: zbus::Connection,
        nm: NetworkManager,
        release: OrbRelease,
        cap: OrbCapabilities,
        connect_timeout: Duration,
        usr_persistent: impl AsRef<Path>,
        profile_store: ProfileStore,
    ) -> Result<Self> {
        let usr_persistent = usr_persistent.as_ref();

        let connd = Self {
            session_dbus,
            nm,
            release,
            cap,
            magic_qr_applied_at: State::new(DateTime::default()),
            connect_timeout,
            profile_store,
        };

        // this must be called before setting up default profiles!
        if let Err(e) = connd.import_stored_profiles().await {
            error!("connd failed to import saved profiles: {e}");
        }

        connd.setup_default_profiles().await?;

        if let Err(e) = connd.import_legacy_wpa_conf(&usr_persistent).await {
            warn!("failed to import legacy wpa config {e}");
        }

        if let Err(e) = connd.ensure_networking_enabled().await {
            warn!("failed to ensure networking is enabled {e}");
        }

        if let Err(e) = connd.ensure_nm_state_below_max_size(usr_persistent).await {
            warn!("failed to ensure nm state below max size: {e}");
        }

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
                .sec(WifiSec::Wpa2Psk)
                .autoconnect(true)
                .hidden(false)
                .priority(-998)
                .add()
                .await?;
        }

        Ok(())
    }

    pub async fn import_stored_profiles(&self) -> Result<()> {
        // this first part of the function is importing old unencrypted NM profiles
        // stored on disk and deleting them. other nm calls to store profiles will only store
        // them in memory going forward
        let old_profiles = self.nm.list_wifi_profiles().await?;
        for profile in old_profiles {
            self.nm.remove_profile(&profile.id).await?;
            self.profile_store.insert(profile);
        }

        self.profile_store.import().await?;

        let mut profiles = self.profile_store.values();
        profiles.sort_by_key(|p| p.priority);

        for profile in profiles {
            self.add_wifi_profile(
                profile.ssid.clone(),
                profile.sec.to_string(),
                profile.psk.clone(),
                profile.hidden,
            )
            .await?;
        }

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
                    self.add_wifi_profile(
                        wpa_conf.ssid,
                        "wpa2".into(),
                        wpa_conf.psk,
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

    pub async fn ensure_nm_state_below_max_size(
        &self,
        usr_persistent: impl AsRef<Path>,
    ) -> Result<()> {
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

        let dir_size = dir_size_kb().await?;
        if dir_size < Self::NM_STATE_MAX_SIZE_KB {
            info!("{nm_dir:?} is below 1024kB. current size {dir_size}kB");
            return Ok(());
        }

        warn!("{nm_dir:?} is above 1024kB. current size {dir_size}kB. attempting to reduce size");

        // remove excess wifi profiles
        let mut wifi_profiles = self.profile_store.values();
        wifi_profiles.sort_by_key(|p| p.priority);

        let profiles_to_keep = 2;
        let profiles_to_remove = wifi_profiles.len().saturating_sub(profiles_to_keep);

        for profile in wifi_profiles.into_iter().take(profiles_to_remove) {
            if profile.id == Self::DEFAULT_WIFI_SSID {
                continue;
            }

            self.nm.remove_profile(&profile.id).await?;
            self.profile_store.remove(&profile.ssid);
        }

        self.profile_store.commit().await?;

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
        info!("adding wifi profile with ssid {ssid}");
        if ssid == Self::DEFAULT_CELLULAR_PROFILE || ssid == Self::DEFAULT_WIFI_SSID {
            return Err(e(&format!("{ssid} is not an allowed SSID name")));
        }

        let sec = match WifiSec::parse(&sec) {
            Some(sec @ (WifiSec::Wpa2Psk | WifiSec::Wpa3Sae)) => sec,
            _ => return Err(e("invalid sec. supported values are Wpa2Psk or Wpa3Sae")),
        };

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

        let prio = self.get_next_priority().await.into_z()?;

        let profile = self
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

        self.profile_store.insert(profile);
        if let Err(e) = self.profile_store.commit().await {
            error!(
                "failed to commit profile store when removing wifi profile. err: {e}"
            );
        }

        info!("profile for ssid: {ssid}, saved successfully");

        Ok(())
    }

    async fn remove_wifi_profile(&self, ssid: String) -> ZResult<()> {
        info!("removing wifi profile with ssid {ssid}");
        if ssid == Self::DEFAULT_CELLULAR_PROFILE || ssid == Self::DEFAULT_WIFI_SSID {
            return Err(e(&format!("{ssid} is not an allowed SSID name",)));
        }

        self.nm.remove_profile(&ssid).await.into_z()?;

        self.profile_store.remove(&ssid);
        if let Err(e) = self.profile_store.commit().await {
            error!(
                "failed to commit profile store when removing wifi profile. err: {e}"
            );
        }

        Ok(())
    }

    async fn list_wifi_profiles(&self) -> ZResult<Vec<orb_connd_dbus::WifiProfile>> {
        let active_conns = self
            .nm
            .active_connections()
            .await
            .inspect_err(|e| warn!("issue retrieving active connections: {e}"))
            .unwrap_or_default();

        let profiles = self
            .nm
            .list_wifi_profiles()
            .await
            .into_z()?
            .into_iter()
            .map(|p| {
                let is_active = active_conns.iter().any(|conn| {
                    conn.id == p.ssid && conn.state == ActiveConnState::Activated
                });

                p.into_dbus_wifi_profile(is_active)
            })
            .collect();

        Ok(profiles)
    }

    async fn scan_wifi(&self) -> ZResult<Vec<orb_connd_dbus::AccessPoint>> {
        let aps = self.nm.wifi_scan().await.into_z()?;
        let profiles = self.nm.list_wifi_profiles().await.into_z()?;
        let active_conns = self
            .nm
            .active_connections()
            .await
            .inspect_err(|e| warn!("issue retrieving active connections: {e}"))
            .unwrap_or_default();

        let aps = aps
            .into_iter()
            .map(|ap| {
                let is_saved = profiles.iter().any(|profile| ap.eq_profile(profile));

                let is_active = active_conns.iter().any(|conn| {
                    conn.id == ap.ssid && conn.state == ActiveConnState::Activated
                });

                ap.into_dbus_ap(is_saved, is_active)
            })
            .collect();

        Ok(aps)
    }

    async fn netconfig_set(
        &self,
        set_wifi: bool,
        set_smart_switching: bool,
        set_airplane_mode: bool,
    ) -> ZResult<orb_connd_dbus::NetConfig> {
        if let OrbCapabilities::WifiOnly = self.cap {
            return Err(eyre!(
                "cannot apply netconfig on orbs that do not have cellular"
            ))
            .into_z();
        }

        info!(
            set_wifi,
            set_smart_switching, set_airplane_mode, "setting netconfig"
        );

        let wifi_enabled = self.nm.wifi_enabled().await.into_z()?;
        let smart_switching_enabled =
            self.nm.smart_switching_enabled().await.into_z()?;

        info!(
            wifi_enabled,
            smart_switching_enabled,
            airplane_mode_enabled = false,
            "current netconfig"
        );

        if wifi_enabled != set_wifi {
            self.nm.set_wifi(set_wifi).await.into_z()?;
        }

        if smart_switching_enabled != set_smart_switching {
            self.nm
                .set_smart_switching(set_smart_switching)
                .await
                .into_z()?;
        }

        if set_airplane_mode {
            warn!("tried applying airplane mode on the orb, but it is not implemented yet!");
        }

        info!("sending netconfig after set");

        Ok(orb_connd_dbus::NetConfig {
            wifi: set_wifi,
            smart_switching: set_smart_switching,
            airplane_mode: false,
        })
    }

    async fn netconfig_get(&self) -> ZResult<orb_connd_dbus::NetConfig> {
        if let OrbCapabilities::WifiOnly = self.cap {
            return Err(eyre!(
                "cannot apply netconfig on orbs that do not have cellular"
            ))
            .into_z();
        }

        info!("getting netconfig");
        let wifi = self.nm.wifi_enabled().await.into_z()?;
        let smart_switching = self.nm.smart_switching_enabled().await.into_z()?;

        info!("sending netconfig after get");
        Ok(orb_connd_dbus::NetConfig {
            wifi,
            smart_switching,
            airplane_mode: false,
        })
    }

    async fn connect_to_wifi(
        &self,
        ssid: String,
    ) -> ZResult<orb_connd_dbus::AccessPoint> {
        info!("connecting to wifi with ssid {ssid}");
        let profiles = self.nm.list_wifi_profiles().await.into_z()?;
        let max_prio = profiles
            .iter()
            .map(|p| p.priority)
            .max()
            .unwrap_or_default();

        let profile = profiles
            .into_iter()
            .find(|p| p.ssid == ssid)
            .wrap_err_with(|| format!("ssid {ssid} is not a saved profile"))
            .into_z()?;

        let aps = self
            .nm
            .wifi_scan()
            .await
            .inspect_err(|e| error!("failed to scan for wifi networks due to err {e}"))
            .into_z()?;

        let active_conns = self.nm.active_connections().await.unwrap_or_default();
        for conn in active_conns {
            if conn.id == profile.id
                && (ActiveConnState::Activated == conn.state
                    || ActiveConnState::Activating == conn.state)
            {
                info!("{:?}, no need to attempt connetion: {conn:?}", conn.state);

                return aps.into_iter().find(|ap| ap.ssid == profile.ssid).map(|ap|ap.into_dbus_ap(true, true)).with_context(|| format!("already connected, but could not find an ap for the connection with ssid {}. should be unreachable state.", profile.ssid)).into_z();
            }
        }

        // We re-add the profile as that will overwrite the old one
        // and is easier than re-using shitty NM d-bus api.
        // We do this to elevate the profile's priority and make sure
        // latest connected profile is always the highest priority one.
        let next_priority = if profile.priority != max_prio {
            self.get_next_priority().await.into_z()?
        } else {
            profile.priority
        };

        let profile = self
            .nm
            .wifi_profile(&profile.id)
            .ssid(&profile.ssid)
            .sec(profile.sec)
            .psk(&profile.psk)
            .priority(next_priority)
            .hidden(profile.hidden)
            .add()
            .await
            .into_z()?;

        let path = profile.path.clone();

        self.profile_store.insert(profile);
        if let Err(e) = self.profile_store.commit().await {
            error!(
                "failed to commit profile store when removing wifi profile. err: {e}"
            );
        }

        for ap in aps {
            if ap.ssid == ssid {
                info!("connecting to ap {ap:?}");

                self.nm
                    .connect_to_wifi(
                        &path,
                        Self::DEFAULT_WIFI_IFACE,
                        self.connect_timeout,
                    )
                    .await
                    .map_err(|e| {
                        eyre!("failed to connect to wifi ssid {ssid} due to err {e}")
                    })
                    .into_z()?;

                info!("successfully connected to ap {ap:?}");

                return Ok(ap.into_dbus_ap(true, true));
            }
        }

        Err(eyre!("could not find ssid {ssid}")).into_z()
    }

    async fn apply_wifi_qr(&self, contents: String) -> ZResult<()> {
        async {
            info!("applying wifi qr code");
            let skip_wifi_qr_restrictions = self.release == OrbRelease::Dev;

            if !skip_wifi_qr_restrictions {
                let state = self.nm.check_connectivity().await.into_z()?;
                let has_no_connectivity = NMConnectivityState::FULL != state;

                let magic_qr_applied_at = self
                    .magic_qr_applied_at
                    .read(|x| *x)
                    .map_err(|_| e("magic qr mtx err"))?;

                let within_magic_qr_timespan = (Utc::now() - magic_qr_applied_at)
                    .num_minutes()
                    < Self::MAGIC_QR_TIMESPAN_MIN;

                let can_apply_wifi_qr = has_no_connectivity || within_magic_qr_timespan;

                if !can_apply_wifi_qr {
                    let msg =
                        "we already have internet connectivity, use signed qr instead";

                    error!(msg);

                    return Err(e(msg));
                }
            }

            let creds = wifi::Credentials::parse(&contents).into_z()?;
            let sec: WifiSec = creds.auth.try_into().into_z()?;

            if let Some(psk) = creds.psk {
                self.add_wifi_profile(
                    creds.ssid.clone(),
                    sec.to_string(),
                    psk.0,
                    creds.hidden,
                )
                .await?;
            }

            self.connect_to_wifi(creds.ssid.clone()).await?;

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

            let connect_result = if let Some(wifi_creds) = netconf.wifi_credentials {
                let sec: WifiSec = wifi_creds.auth.try_into().into_z()?;
                info!(ssid = wifi_creds.ssid, "adding wifi network from netconfig");

                if let Some(psk) = wifi_creds.psk {
                    self.add_wifi_profile(
                        wifi_creds.ssid.clone(),
                        sec.to_string(),
                        psk.0,
                        wifi_creds.hidden,
                    )
                    .await?;
                }

                self.connect_to_wifi(wifi_creds.ssid.clone())
                    .await
                    .map(|_| ())
            } else {
                Ok(())
            };

            // Orbs without cellular do not support extra NetConfig fields
            if self.cap == OrbCapabilities::WifiOnly {
                return connect_result;
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

            connect_result
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

    async fn connection_state(&self) -> ZResult<ConnectionState> {
        let uri = self.nm.connectivity_check_uri().await.into_z()?;

        info!("checking connectivity against {uri}");

        self.nm.check_connectivity().await.into_z()?;
        let value = self.nm.state().await.into_z()?;

        use ConnectionState::*;
        let state = match value {
            NMState::UNKNOWN | NMState::ASLEEP | NMState::DISCONNECTED => Disconnected,

            NMState::DISCONNECTING => Disconnecting,

            NMState::CONNECTING => Connecting,

            NMState::CONNECTED_LOCAL | NMState::CONNECTED_SITE => PartiallyConnected,

            NMState::CONNECTED_GLOBAL => Connected,
        };

        info!("connection state: {state:?}");

        Ok(state)
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

impl WifiProfile {
    fn into_dbus_wifi_profile(self, is_active: bool) -> orb_connd_dbus::WifiProfile {
        orb_connd_dbus::WifiProfile {
            ssid: self.ssid,
            sec: self.sec.to_string(),
            psk: self.psk,
            is_active,
        }
    }
}

impl AccessPoint {
    fn into_dbus_ap(
        self,
        is_saved: bool,
        is_active: bool,
    ) -> orb_connd_dbus::AccessPoint {
        use NM80211Mode::*;
        let mode = match self.mode {
            UNKNOWN => "Unknown",
            ADHOC => "Adhoc",
            INFRA => "Infra",
            AP => "Ap",
            MESH => "Mesh",
        }
        .to_string();

        let capabiltiies = orb_connd_dbus::AccessPointCapabilities {
            privacy: self.capabilities.privacy,
            wps: self.capabilities.wps,
            wps_pbc: self.capabilities.wps_pbc,
            wps_pin: self.capabilities.wps_pin,
        };

        orb_connd_dbus::AccessPoint {
            ssid: self.ssid,
            bssid: self.bssid,
            is_saved,
            is_active,
            freq_mhz: self.freq_mhz,
            max_bitrate_kbps: self.max_bitrate_kbps,
            strength_pct: self.strength_pct,
            last_seen: self.last_seen.to_rfc3339(),
            mode,
            capabilities: capabiltiies,
            sec: self.sec.to_string(),
        }
    }
}
