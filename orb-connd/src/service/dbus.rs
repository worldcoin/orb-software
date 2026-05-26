use crate::{
    conn_http_check::ConnHttpCheck,
    network_manager::{self, WifiSec},
    service::{netconfig::NetConfig, wifi, ConndService},
    utils::IntoZResult,
    OrbCapabilities,
};
use async_trait::async_trait;
use chrono::Utc;
use color_eyre::eyre::eyre;
use orb_connd_dbus::{ConndT, ConnectionState};
use orb_info::orb_os_release::OrbRelease;
use rusty_network_manager::dbus_interface_types::NMConnectivityState;
use tracing::{error, info, warn};
use zbus::fdo::{Error as ZErr, Result as ZResult};

#[async_trait]
impl ConndT for ConndService {
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

            self.connect_to_wifi(&creds.ssid).await.into_z()?;

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

                self.connect_to_wifi(&wifi_creds.ssid).await.map(|_| ())
            } else {
                Ok(())
            };

            // Orbs without cellular do not support extra NetConfig fields
            if self.cap == OrbCapabilities::WifiOnly {
                return connect_result.into_z();
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

            connect_result.into_z()
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
        let uri = self.nm.connectivity_check_uri().await.into_z()?;
        info!("checking connectivity against {uri}");

        self.nm.check_connectivity().await.into_z()?;
        let nm_state = self.nm.state().await.into_z()?;
        let res = ConnHttpCheck::run(&uri, None).await;

        info!("nm state: {nm_state:?}, http conn check: {res:?}");

        let conn_state = if res.is_ok_and(|r| {
            r.status.is_success()
                && r.nm_status.is_some_and(|status| status == "online")
        }) {
            ConnectionState::Connected
        } else {
            ConnectionState::from(nm_state)
        };

        Ok(conn_state)
    }
}

fn e(str: &str) -> ZErr {
    ZErr::Failed(str.to_string())
}

impl From<network_manager::ConnectionState> for ConnectionState {
    fn from(value: network_manager::ConnectionState) -> Self {
        use network_manager::ConnectionState::*;
        match value {
            Disconnected => ConnectionState::Disconnected,
            Disconnecting => ConnectionState::Disconnecting,
            Connecting => ConnectionState::Connecting,
            PartiallyConnected => ConnectionState::PartiallyConnected,
            Connected => ConnectionState::Connected,
        }
    }
}
