use crate::{
    network_manager::{AccessPoint, ActiveConnState, WifiProfile, WifiSec},
    service::ConndService,
};
use color_eyre::{eyre::eyre, Result};
use rusty_network_manager::dbus_interface_types::NM80211Mode;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{error, info, warn};
use zenorb::{zenoh::query::Query, zoci::ZociQueryExt};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
struct WifiAdd {
    ssid: String,
    sec: String,
    pwd: String,
    #[serde(default)]
    hidden: bool,
    #[serde(default)]
    join_now: bool,
}

pub async fn wifi_add(connd: ConndService, query: Query) -> Result<()> {
    let response = async {
        let WifiAdd {
            ssid,
            sec,
            pwd,
            hidden,
            join_now,
        } = query.json()?;

        info!("adding wifi profile with ssid {ssid}");

        let sec = match WifiSec::parse(&sec) {
            Some(sec @ (WifiSec::Wpa2Psk | WifiSec::Wpa3Sae)) => sec,
            _ => {
                return Err(eyre!(
                    "invalid sec. supported values are Wpa2Psk or Wpa3Sae"
                ))
            }
        };

        connd.wifi_profile_add(&ssid, sec, &pwd, hidden).await?;

        if let Err(e) = connd.commit_profiles_to_storage().await {
            error!(error = ?e,
                "failed to commit profile store when adding wifi profile. err: {e}"
            );
        }

        info!("profile for ssid: {ssid}, saved successfully");

        let connect_result = if join_now {
            let network = connd.connect_to_wifi(&ssid).await.map(|ap|AccessPointDto::from_domain(ap,true, true));
            Some(network)
        } else {
            None
        };

        let response = match connect_result {
            None => json!({ "connection_success": null, "network": null }),

            Some(Ok(network)) => {
                json!({ "connection_success": true, "network": network })
            }

            Some(Err(e)) => {
                warn!("failed to connect to network {ssid}, removing it from saved profiles");

                // if we fail to connect, delete the profile
                // not the best place for this but it is what it is for now -vmenge
                if let Err(e) = connd.remove_wifi_profile(&ssid).await {
                    warn!("failed to remove ssid {ssid} after failed connection attempt. err: {e}");
                }

                json!({ "connection_success": false, "error": e.to_string() })
            }
        };

        Ok(response)
    }
    .await
    .map_err(|e| e.to_string());

    query.res(response).await?;

    Ok(())
}

pub async fn wifi_connect(connd: ConndService, query: Query) -> Result<()> {
    let response = async {
        let ssid = query.payload_str()?;
        connd
            .connect_to_wifi(&ssid)
            .await
            .map(|ap| AccessPointDto::from_domain(ap, true, true))
    }
    .await
    .map_err(|e| e.to_string());

    query.res(response).await?;

    Ok(())
}

pub async fn wifi_list(connd: ConndService, query: Query) -> Result<()> {
    info!("listing wifi profiles");

    let active_conns = connd
        .nm
        .active_connections()
        .await
        .inspect_err(|e| warn!("issue retrieving active connections: {e}"))
        .unwrap_or_default();

    let profiles = connd
        .nm
        .list_wifi_profiles()
        .await
        .map(|ps| {
            ps.into_iter()
                .map(|p| {
                    let is_active = active_conns.iter().any(|conn| {
                        conn.id == p.ssid && conn.state == ActiveConnState::Activated
                    });

                    WifiProfileDto::from_domain(p, is_active)
                })
                .collect::<Vec<_>>()
        })
        .map_err(|e| e.to_string());

    query.res(profiles).await?;

    Ok(())
}

pub async fn wifi_scan(connd: ConndService, query: Query) -> Result<()> {
    let response = connd
        .wifi_scan()
        .await
        .map(|aps| {
            aps.into_iter()
                .map(|(ap, is_saved, is_active)| {
                    AccessPointDto::from_domain(ap, is_saved, is_active)
                })
                .collect::<Vec<_>>()
        })
        .map_err(|e| e.to_string());

    query.res(response).await?;

    Ok(())
}

pub async fn wifi_remove(connd: ConndService, query: Query) -> Result<()> {
    let response = async {
        let ssid = query.payload_str()?;
        connd.remove_wifi_profile(&ssid).await?;

        Ok::<_, color_eyre::Report>(())
    }
    .await
    .map_err(|e| e.to_string());

    query.res(response).await?;

    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WifiProfileDto {
    pub ssid: String,
    pub sec: String,
    pub is_active: bool,
}

impl WifiProfileDto {
    fn from_domain(profile: WifiProfile, is_active: bool) -> WifiProfileDto {
        WifiProfileDto {
            ssid: profile.ssid,
            sec: profile.sec.to_string(),
            is_active,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccessPointDto {
    pub ssid: String,
    pub bssid: String,
    pub is_saved: bool,
    pub freq_mhz: u32,
    pub max_bitrate_kbps: u32,
    pub strength_pct: u8,
    pub last_seen: String,
    pub mode: String,
    pub capabilities: AccessPointCapabilitiesDto,
    pub sec: String,
    pub is_active: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct AccessPointCapabilitiesDto {
    /// WEP/WPA/WPA2/3 required (not "open")
    pub privacy: bool,
    /// WPS supported
    pub wps: bool,
    /// WPS push-button
    pub wps_pbc: bool,
    /// WPS PIN
    pub wps_pin: bool,
}

impl AccessPointDto {
    fn from_domain(ap: AccessPoint, is_saved: bool, is_active: bool) -> AccessPointDto {
        use NM80211Mode::*;
        let mode = match ap.mode {
            UNKNOWN => "Unknown",
            ADHOC => "Adhoc",
            INFRA => "Infra",
            AP => "Ap",
            MESH => "Mesh",
        }
        .to_string();

        let capabiltiies = AccessPointCapabilitiesDto {
            privacy: ap.capabilities.privacy,
            wps: ap.capabilities.wps,
            wps_pbc: ap.capabilities.wps_pbc,
            wps_pin: ap.capabilities.wps_pin,
        };

        AccessPointDto {
            ssid: ap.ssid,
            bssid: ap.bssid,
            is_saved,
            is_active,
            freq_mhz: ap.freq_mhz,
            max_bitrate_kbps: ap.max_bitrate_kbps,
            strength_pct: ap.strength_pct,
            last_seen: ap.last_seen.to_rfc3339(),
            mode,
            capabilities: capabiltiies,
            sec: ap.sec.to_string(),
        }
    }
}
