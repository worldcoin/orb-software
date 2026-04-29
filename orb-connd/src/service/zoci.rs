use crate::{
    network_manager::{AccessPoint, WifiSec},
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
            let network = connd.connect_to_wifi(&ssid).await.map(|n|n.into_dbus_ap(true, true));
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
            .map(|ap| ap.into_dbus_ap(true, true))
    }
    .await
    .map_err(|e| e.to_string());

    query.res(response).await?;

    Ok(())
}

pub async fn wifi_list(_connd: ConndService, _query: Query) -> Result<()> {
    todo!("unimplmented")
}

pub async fn wifi_scan(connd: ConndService, query: Query) -> Result<()> {
    let response = connd
        .wifi_scan()
        .await
        .map(|aps| {
            aps.into_iter()
                .map(|(ap, is_saved, is_active)| ap.into_dbus_ap(is_saved, is_active))
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

pub async fn active_conns(_connd: ConndService, _query: Query) -> Result<()> {
    todo!("unimplmented")
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
