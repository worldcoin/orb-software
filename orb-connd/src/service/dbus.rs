use std::time::{Duration, Instant};

use crate::{
    network_manager::{AccessPoint, ActiveConnState, WifiProfile, WifiSec},
    service::{netconfig::NetConfig, wifi, ConndService},
    utils::IntoZResult,
    OrbCapabilities,
};
use async_trait::async_trait;
use chrono::Utc;
use color_eyre::eyre::{eyre, ContextCompat};
use orb_connd_dbus::{ConndT, ConnectionState};
use orb_info::orb_os_release::OrbRelease;
use rusty_network_manager::dbus_interface_types::{
    NM80211Mode, NMConnectivityState, NMState,
};
use tokio::time;
use tracing::{error, info, warn};
use zbus::fdo::{Error as ZErr, Result as ZResult};

#[async_trait]
impl ConndT for ConndService {
    /// d-bus impl
    async fn add_wifi_profile(
        &self,
        ssid: String,
        sec: String,
        pwd: String,
        hidden: bool,
    ) -> ZResult<()> {
        async {
            info!("adding wifi profile with ssid {ssid}");

            let sec = match WifiSec::parse(&sec) {
                Some(sec @ (WifiSec::Wpa2Psk | WifiSec::Wpa3Sae)) => sec,
                _ => {
                    return Err(e(
                        "invalid sec. supported values are Wpa2Psk or Wpa3Sae",
                    ))
                }
            };

            self.wifi_profile_add(&ssid, sec, &pwd, hidden)
                .await
                .into_z()?;

            if let Err(e) = self.commit_profiles_to_storage().await {
                error!(error = ?e,
                    "failed to commit profile store when adding wifi profile. err: {e}"
                );
            }

            info!("profile for ssid: {ssid}, saved successfully");

            Ok(())
        }
        .await
        .inspect_err(|e| error!(error = ?e, "failed to add wifi profile: {e}"))
    }

    /// d-bus impl
    async fn remove_wifi_profile(&self, ssid: String) -> ZResult<()> {
        info!("removing wifi profile with ssid {ssid}");
        if ssid == Self::DEFAULT_CELLULAR_PROFILE || ssid == Self::DEFAULT_WIFI_SSID {
            return Err(e(&format!("{ssid} is not an allowed SSID name",)));
        }

        self.nm.remove_profile(&ssid).await.into_z()?;

        if let Err(e) = self.commit_profiles_to_storage().await {
            error!(
                "failed to commit profile store when removing wifi profile. err: {e}"
            );
        }

        Ok(())
    }

    /// d-bus impl
    async fn list_wifi_profiles(&self) -> ZResult<Vec<orb_connd_dbus::WifiProfile>> {
        info!("listing wifi profiles");

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

    /// d-bus impl
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

    /// d-bus impl
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

    /// d-bus impl
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

    /// d-bus impl
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

        let get_activated_or_activating_conn = async || {
            self.nm
                .active_connections()
                .await
                .unwrap_or_default()
                .into_iter()
                .find(|conn| {
                    conn.id == profile.id
                        && (conn.state == ActiveConnState::Activated
                            || conn.state == ActiveConnState::Activating)
                })
        };

        let start = Instant::now();
        let timeout = Duration::from_secs(10);
        let backoff = Duration::from_secs(2);
        while let Some(conn) = get_activated_or_activating_conn().await {
            if ActiveConnState::Activated == conn.state {
                info!("{:?}, no need to attempt connetion: {conn:?}", conn.state);

                return aps.into_iter().
                    find(|ap| ap.ssid == profile.ssid).map(|ap|ap.into_dbus_ap(true, true)).with_context(|| format!("already connected, but could not find an ap for the connection with ssid {}. should be unreachable state.", profile.ssid)).into_z();
            }

            // only possible state left is ActiveConnState::Activating
            if start.elapsed() > timeout {
                warn!("{:?}, connection spent too long activating, will re-add it {conn:?}", conn.state);
                break;
            }

            info!(
                "{:?} connection still activating, waiting {}s and trying again",
                conn.state,
                backoff.as_secs()
            );

            time::sleep(backoff).await;
        }

        info!(
            "no active or activating conn, configuring new conn to profile {}",
            profile.id
        );

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

        if let Err(e) = self.commit_profiles_to_storage().await {
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

    /// d-bus impl
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
                self.wifi_profile_add(&creds.ssid, sec, &psk.0, creds.hidden)
                    .await
                    .into_z()?;

                if let Err(e) = self.commit_profiles_to_storage().await {
                    error!("failed to commit saved profiles: {e}");
                }
            }

            self.connect_to_wifi(creds.ssid.clone()).await?;

            info!("applied wifi qr successfully");

            Ok(())
        }
        .await
        .inspect_err(|e| error!("failed to apply wifi qr with {e}"))
    }

    /// d-bus impl
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
                    self.wifi_profile_add(
                        &wifi_creds.ssid,
                        sec,
                        &psk.0,
                        wifi_creds.hidden,
                    )
                    .await
                    .into_z()?;

                    if let Err(e) = self.commit_profiles_to_storage().await {
                        error!("failed to commit saved profiles: {e}");
                    }
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

    /// d-bus impl
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

        if let Err(e) = self.commit_profiles_to_storage().await {
            error!(
                "failed to commit profile store when applying magic reset qr. err: {e}"
            );
        }

        info!("successfuly applied magic reset qr");

        Ok(())
    }

    /// d-bus impl
    async fn connection_state(&self) -> ZResult<ConnectionState> {
        // let uri = self.nm.connectivity_check_uri().await.into_z()?;

        // info!("checking connectivity against {uri}");

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

        // info!("connection state: {state:?}");

        Ok(state)
    }
}

fn e(str: &str) -> ZErr {
    ZErr::Failed(str.to_string())
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
