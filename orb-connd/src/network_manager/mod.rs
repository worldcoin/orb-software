use bon::bon;
use chrono::{DateTime, Utc};
use color_eyre::{
    eyre::{bail, ContextCompat},
    Result,
};
use derive_more::Display;
use rusty_network_manager::{
    dbus_interface_types::{NM80211Mode, NMDeviceType},
    AccessPointProxy, ActiveProxy, DeviceProxy, NM80211ApFlags, NM80211ApSecurityFlags,
    NetworkManagerProxy, SettingsConnectionProxy, SettingsProxy, WirelessProxy,
};
use std::collections::HashMap;
use tokio::fs;
use tracing::warn;
use zbus::zvariant::{Array, ObjectPath, OwnedObjectPath, OwnedValue, Value};

#[derive(Clone)]
pub struct NetworkManager {
    conn: zbus::Connection,
}

#[bon]
impl NetworkManager {
    pub fn new(conn: zbus::Connection) -> Self {
        Self { conn }
    }

    pub async fn primary_connection(&self) -> Result<Option<Connection>> {
        let nm = NetworkManagerProxy::new(&self.conn).await?;
        let ac_path = nm.primary_connection().await?;

        if ac_path.as_str() == "/" {
            return Ok(None);
        }

        let ac = ActiveProxy::new_from_path(ac_path, &self.conn).await?;
        if !ac.default().await? && !ac.default6().await? {
            bail!("no default (IPv4/IPv6) route owned by the primary connection");
        }

        let netkind = ac.type_().await?;
        let netkind = NetworkKind::parse(&netkind)
            .wrap_err_with(|| format!("{netkind} is not a valid NetworkKind"))?;

        let conn = match netkind {
            NetworkKind::Wifi => {
                let ap = AccessPointProxy::new_from_path(
                    ac.specific_object().await?,
                    &self.conn,
                )
                .await?;
                let ssid = String::from_utf8_lossy(&ap.ssid().await?).into_owned();

                Connection::Wifi { ssid }
            }

            NetworkKind::Cellular => {
                let settings = SettingsConnectionProxy::new_from_path(
                    ac.connection().await?,
                    &self.conn,
                )
                .await?
                .get_settings()
                .await?;

                let apn = settings.get("gsm").and_then(|gsm| {
                    gsm.get("apn")?
                        .downcast_ref()
                        .ok()
                        .filter(|apn: &String| !apn.is_empty())
                }).wrap_err("could not retrieve apn information from active cellular connection")?;

                Connection::Cellular { apn }
            }
        };

        Ok(Some(conn))
    }

    /// Connects to an already existing wifi profile
    pub async fn connect_to_wifi(
        &self,
        profile_obj_path: &str,
        iface: &str,
    ) -> Result<()> {
        let nm = NetworkManagerProxy::new(&self.conn).await?;

        nm.activate_connection(
            &ObjectPath::try_from(profile_obj_path)?,
            &self.find_device(iface).await?.as_ref(),
            &ObjectPath::try_from("/")?,
        )
        .await?;

        Ok(())
    }

    pub async fn list_wifi_profiles(&self) -> Result<Vec<WifiProfile>> {
        let settings = SettingsProxy::new(&self.conn).await?;
        let paths = settings.list_connections().await?;

        let mut out = Vec::with_capacity(paths.len());
        for path in paths {
            let cp = SettingsConnectionProxy::new_from_path(path.clone(), &self.conn)
                .await?;

            let settings = cp.get_settings().await?;
            let secrets = cp
                .get_secrets("802-11-wireless-security")
                .await
                .unwrap_or_default();

            if let Some(profile) = WifiProfile::from_dbus(&path, &settings, &secrets) {
                out.push(profile);
            }
        }

        Ok(out)
    }

    pub async fn list_cellular_profiles(&self) -> Result<Vec<CellularProfile>> {
        let settings = SettingsProxy::new(&self.conn).await?;
        let paths = settings.list_connections().await?;

        let mut out = Vec::with_capacity(paths.len());
        for path in paths {
            let cp = SettingsConnectionProxy::new_from_path(path.clone(), &self.conn)
                .await?;
            let settings = cp.get_settings().await?;

            if let Some(profile) = CellularProfile::from_dbus(&path, &settings) {
                out.push(profile);
            }
        }

        Ok(out)
    }

    pub async fn wifi_scan(&self) -> Result<Vec<AccessPoint>> {
        let nm = NetworkManagerProxy::new(&self.conn).await?;
        let devices = nm.get_all_devices().await?;

        let mut access_points = Vec::new();

        for dev_path in devices {
            let dvc = DeviceProxy::new_from_path(dev_path.clone(), &self.conn).await?;
            let dev_type = dvc.device_type().await?;

            let Ok(dev_type) = NMDeviceType::try_from(dev_type) else {
                warn!("failed to parse NMDeviceType from {dev_type}");
                continue;
            };

            if dev_type != NMDeviceType::WIFI {
                continue;
            }

            let wifi =
                WirelessProxy::new_from_path(dev_path.clone(), &self.conn).await?;

            // @vmenge: unsure if the scan throttle on nm side might cause a failure here
            // if so we move on and just used cached networks
            if let Err(e) = wifi.request_scan(Default::default()).await {
                warn!("nm failed to rescan wifi networks, err: {e}");
            }

            let ap_paths = wifi
                .get_all_access_points()
                .await
                .inspect_err(|e| {
                    warn!("failed to get access points for dev {dev_path} with err {e}")
                })
                .unwrap_or_default();

            for ap_path in ap_paths {
                let ap = AccessPointProxy::new_from_path(ap_path.clone(), &self.conn)
                    .await?;

                let ssid = ap.ssid().await?;
                let ssid = String::from_utf8_lossy(&ssid).into_owned();
                let freq_mhz = ap.frequency().await?;
                let bssid = ap.hw_address().await?;
                let last_seen = ap.last_seen().await?;
                let last_seen = last_seen_to_utc(last_seen).await?;

                let max_bitrate_kbps = ap.max_bitrate().await?;
                let mode = NM80211Mode::try_from(ap.mode().await?)?;
                let strength_pct = ap.strength().await?;

                let flags = ap.flags().await?;
                let flags = NM80211ApFlags::from_bits_truncate(flags);
                let capabilities = ApCap::from(flags);

                let rsn = ap.rsn_flags().await?;
                let rsn = NM80211ApSecurityFlags::from_bits_truncate(rsn);
                let wpa = ap.wpa_flags().await?;
                let wpa = NM80211ApSecurityFlags::from_bits_truncate(wpa);
                let sec = WifiSec::from_flags(wpa, rsn);

                let access_point = AccessPoint {
                    ssid,
                    bssid,
                    freq_mhz,
                    max_bitrate_kbps,
                    strength_pct,
                    last_seen,
                    mode,
                    capabilities,
                    sec,
                };

                access_points.push(access_point);
            }
        }

        Ok(access_points)
    }

    /// Adds a wifi profile ensuring id uniqueness
    #[builder(finish_fn=add)]
    pub async fn wifi_profile(
        &self,
        #[builder(start_fn)] id: &str,
        ssid: &str,
        sec: WifiSec,
        psk: &str,
        #[builder(default = true)] autoconnect: bool,
        #[builder(default = 0)] priority: i32,
        #[builder(default = false)] hidden: bool,
        #[builder(default = 0)] max_autoconnect_retries: u64, // 0 here is infinite
    ) -> Result<OwnedObjectPath> {
        self.remove_profile(id).await?;

        let connection = HashMap::from_iter([
            kv("id", id),
            kv("type", "802-11-wireless"),
            kv("autoconnect", autoconnect),
            kv("autoconnect-priority", priority),
            kv("autoconnect-retries", max_autoconnect_retries),
        ]);

        let wifi = HashMap::from_iter([
            kv("ssid", ssid.as_bytes()),
            kv("mode", "infrastructure"),
            kv("hidden", hidden),
        ]);

        let sec = HashMap::from_iter([kv("key-mgmt", sec.as_nm_str()), kv("psk", psk)]);
        let ipv4 = HashMap::from_iter([kv("method", "auto")]);
        let ipv6 = HashMap::from_iter([kv("method", "ignore")]);

        let settings = HashMap::from_iter([
            ("connection", connection),
            ("802-11-wireless", wifi),
            ("802-11-wireless-security", sec),
            ("ipv4", ipv4),
            ("ipv6", ipv6),
        ]);

        let sp = SettingsProxy::new(&self.conn).await?;
        let path = sp.add_connection(settings).await?;

        Ok(path)
    }

    /// Adds a cellular profile ensuring id uniqueness
    #[builder(finish_fn=add)]
    pub async fn cellular_profile(
        &self,
        #[builder(start_fn)] id: &str,
        apn: &str,
        iface: &str,
        #[builder(default = 0)] priority: i32,
        #[builder(default = 0)] max_autoconnect_retries: u64, // 0 here is infinite
    ) -> Result<()> {
        self.remove_profile(id).await?;

        let connection = HashMap::from_iter([
            kv("id", id),
            kv("type", "gsm"),
            kv("interface-name", iface),
            kv("autoconnect", true),
            kv("autoconnect-priority", priority),
            kv("autoconnect-retries", max_autoconnect_retries),
        ]);

        let gsm = HashMap::from_iter([kv("apn", apn)]);
        let ipv4 = HashMap::from_iter([kv("method", "auto")]);
        let ipv6 = HashMap::from_iter([kv("method", "ignore")]);

        let settings = HashMap::from_iter([
            ("connection", connection),
            ("gsm", gsm),
            ("ipv4", ipv4),
            ("ipv6", ipv6),
        ]);

        let sp = SettingsProxy::new(&self.conn).await?;
        sp.add_connection(settings).await?;

        Ok(())
    }

    pub async fn remove_profile(&self, id_or_uuid: &str) -> Result<bool> {
        let settings = SettingsProxy::new(&self.conn).await?;
        let paths = settings.list_connections().await?;

        let mut deleted = false;

        for path in paths {
            let conn = SettingsConnectionProxy::new_from_path(path.clone(), &self.conn)
                .await?;
            let s = conn.get_settings().await?;

            if let Some(conn_map) = s.get("connection") {
                let id = v_str(conn_map, "id");
                let uuid = v_str(conn_map, "uuid");

                if id.as_deref() == Some(id_or_uuid)
                    || uuid.as_deref() == Some(id_or_uuid)
                {
                    conn.delete().await?;
                    deleted = true;
                }
            }
        }

        Ok(deleted)
    }

    pub async fn set_smart_switching(&self, on: bool) -> Result<()> {
        let nm = NetworkManagerProxy::new(&self.conn).await?;
        nm.set_connectivity_check_enabled(on).await?;
        Ok(())
    }

    pub async fn set_wifi(&self, on: bool) -> Result<()> {
        let nm = NetworkManagerProxy::new(&self.conn).await?;
        nm.set_wireless_enabled(on).await?;
        Ok(())
    }

    pub async fn smart_switching_enabled(&self) -> Result<bool> {
        let nm = NetworkManagerProxy::new(&self.conn).await?;
        let enabled = nm.connectivity_check_enabled().await?;
        Ok(enabled)
    }

    pub async fn wifi_enabled(&self) -> Result<bool> {
        let nm = NetworkManagerProxy::new(&self.conn).await?;
        let enabled = nm.wireless_enabled().await?;
        Ok(enabled)
    }

    pub async fn set_networking(&self, on: bool) -> Result<()> {
        let nm = NetworkManagerProxy::new(&self.conn).await?;
        nm.enable(on).await?;
        Ok(())
    }

    pub async fn networking_enabled(&self) -> Result<bool> {
        let nm = NetworkManagerProxy::new(&self.conn).await?;
        let enabled = nm.networking_enabled().await?;
        Ok(enabled)
    }

    pub async fn set_wwan(&self, on: bool) -> Result<()> {
        let nm = NetworkManagerProxy::new(&self.conn).await?;
        nm.set_wwan_enabled(on).await?;
        Ok(())
    }

    pub async fn wwan_enabled(&self) -> Result<bool> {
        let nm = NetworkManagerProxy::new(&self.conn).await?;
        let enabled = nm.wwan_enabled().await?;
        Ok(enabled)
    }

    async fn find_device(&self, dev_name: &str) -> Result<OwnedObjectPath> {
        let nm = NetworkManagerProxy::new(&self.conn).await?;

        let mut dev_path: Option<OwnedObjectPath> = None;
        for p in nm.devices().await? {
            let d = DeviceProxy::builder(&self.conn)
                .path(p.clone())?
                .build()
                .await?;

            if d.interface().await.unwrap_or_default() == dev_name {
                dev_path = Some(p);
                break;
            }
        }

        let dev_path = dev_path.context("wifi device not found")?;

        Ok(dev_path)
    }

    pub async fn has_connectivity(&self) -> Result<bool> {
        let nm = NetworkManagerProxy::new(&self.conn).await?;
        let connectivity = nm.check_connectivity().await?;

        Ok(connectivity == 4)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Connection {
    Cellular { apn: String },
    Wifi { ssid: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkKind {
    Wifi,     // "802-11-wireless"
    Cellular, // "gsm"
}

#[derive(Debug, Clone)]
pub struct WifiProfile {
    pub id: String,
    pub uuid: String,
    pub ssid: String,
    pub sec: WifiSec,
    pub psk: String,
    pub autoconnect: bool,
    pub priority: i32,
    pub hidden: bool,
    pub path: String,
}

#[derive(Debug, Clone)]
pub struct CellularProfile {
    pub id: String,
    pub uuid: String,
    pub apn: String,
    pub iface: String,
    pub path: String,
}

impl NetworkKind {
    pub fn parse(s: &str) -> Option<NetworkKind> {
        match s {
            "802-11-wireless" => Some(NetworkKind::Wifi),
            "gsm" => Some(NetworkKind::Cellular),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            NetworkKind::Cellular => "gsm",
            NetworkKind::Wifi => "802-11-wireless",
        }
    }
}

impl WifiProfile {
    pub fn from_dbus(
        path: &OwnedObjectPath,
        settings: &HashMap<String, HashMap<String, OwnedValue>>,
        secrets: &HashMap<String, HashMap<String, OwnedValue>>,
    ) -> Option<WifiProfile> {
        let conn = settings.get("connection")?;
        let kind = v_str(conn, "type")
            .as_deref()
            .and_then(NetworkKind::parse)?;

        if kind != NetworkKind::Wifi {
            return None;
        }

        let id = v_str(conn, "id")?;
        let uuid = v_str(conn, "uuid")?;

        let autoconnect = conn
            .get("autoconnect")
            .and_then(|ac| ac.downcast_ref().ok())
            .unwrap_or(true);

        let priority = conn
            .get("autoconnect-priority")
            .and_then(|ac| ac.downcast_ref().ok())
            .unwrap_or_default();

        let wifi = settings.get("802-11-wireless")?;
        let ssid: Array<'_> = wifi.get("ssid")?.downcast_ref().ok()?;
        let ssid: Vec<u8> = ssid.try_into().ok()?;
        let ssid = String::from_utf8_lossy(&ssid).to_string();
        let hidden = wifi
            .get("hidden")
            .and_then(|v| v.downcast_ref().ok())
            .unwrap_or_default();

        let sec_map = settings.get("802-11-wireless-security");
        let sec = sec_map
            .and_then(|m| v_str(m, "key-mgmt"))
            .as_deref()
            .and_then(WifiSec::parse)?;

        let pwd = secrets
            .get("802-11-wireless-security")
            .and_then(|m| v_str(m, "psk"))?;

        Some(WifiProfile {
            id,
            uuid,
            ssid,
            sec,
            psk: pwd,
            autoconnect,
            priority,
            hidden,
            path: path.to_string(),
        })
    }
}

impl CellularProfile {
    pub fn from_dbus(
        path: &OwnedObjectPath,
        settings: &HashMap<String, HashMap<String, OwnedValue>>,
    ) -> Option<Self> {
        let conn = settings.get("connection")?;
        let kind = v_str(conn, "type")
            .as_deref()
            .and_then(NetworkKind::parse)?;

        if kind != NetworkKind::Cellular {
            return None;
        }

        let id = v_str(conn, "id")?;
        let uuid = v_str(conn, "uuid")?;
        let iface = v_str(conn, "interface-name")?;

        let gsm = settings.get("gsm")?;
        let apn = v_str(gsm, "apn")?;

        Some(CellularProfile {
            id,
            uuid,
            apn,
            iface,
            path: path.to_string(),
        })
    }
}

fn kv<'a, T>(key: &'a str, val: T) -> (&'a str, Value<'a>)
where
    T: Into<Value<'a>>,
{
    (key, val.into())
}

fn v_str(map: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    map.get(key)?.downcast_ref().ok()
}

#[derive(Debug, Clone, PartialEq)]
pub struct AccessPoint {
    pub ssid: String,
    pub bssid: String,
    pub freq_mhz: u32,
    pub max_bitrate_kbps: u32,
    pub strength_pct: u8,
    pub last_seen: DateTime<Utc>,
    pub mode: NM80211Mode,
    pub capabilities: ApCap,
    pub sec: WifiSec,
}

async fn last_seen_to_utc(last_seen: i32) -> Result<DateTime<Utc>> {
    if last_seen < 0 {
        bail!("last seen is less than 0. last_seen: {last_seen}");
    }

    let s = fs::read_to_string("/proc/stat").await?;
    let boot: i64 = s
        .lines()
        .find_map(|l| l.strip_prefix("btime ")?.trim().parse().ok())
        .wrap_err("failed to find boottime in /proc/stat")?;

    let ts = boot + last_seen as i64;
    let dt = DateTime::<Utc>::from_timestamp(ts, 0)
        .wrap_err_with(|| format!("failed parsing datetime. ts used: {ts}"))?;

    Ok(dt)
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ApCap {
    /// WEP/WPA/WPA2/3 required (not "open")
    pub privacy: bool,
    /// WPS supported
    pub wps: bool,
    /// WPS push-button
    pub wps_pbc: bool,
    /// WPS PIN
    pub wps_pin: bool,
}

impl From<NM80211ApFlags> for ApCap {
    fn from(f: NM80211ApFlags) -> Self {
        Self {
            privacy: f.contains(NM80211ApFlags::PRIVACY),
            wps: f.contains(NM80211ApFlags::WPS),
            wps_pbc: f.contains(NM80211ApFlags::WPS_PBC),
            wps_pin: f.contains(NM80211ApFlags::WPS_PIN),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, Hash)]
pub enum WifiSec {
    /// No protection (or RSN IE present but no auth/key-mgmt required).
    Open,
    /// Enhanced Open (OWE): opportunistic encryption without authentication.
    Owe,
    /// OWE transition mode: AP advertises open + OWE BSSID pair.
    OweTransition,
    /// Legacy WEP (avoid).
    Wep,
    /// WPA1 with PSK (legacy).
    Wpa1Psk,
    /// WPA1 with 802.1X/EAP (legacy enterprise).
    Wpa1Eap,
    /// WPA2-Personal (PSK).
    Wpa2Psk,
    /// WPA3-Personal (SAE).
    Wpa3Sae,
    /// WPA2/WPA3 mixed (PSK + SAE).
    Wpa2Wpa3Transitional,
    /// WPA2/3-Enterprise (802.1X/EAP).
    Enterprise,
    /// Couldn’t classify from flags.
    Unknown,
}

impl WifiSec {
    pub fn from_flags(
        wpa: NM80211ApSecurityFlags,
        rsn: NM80211ApSecurityFlags,
    ) -> Self {
        if rsn.is_empty() {
            let wep = wpa.intersects(
                NM80211ApSecurityFlags::PAIR_WEP40
                    | NM80211ApSecurityFlags::PAIR_WEP104
                    | NM80211ApSecurityFlags::GROUP_WEP40
                    | NM80211ApSecurityFlags::GROUP_WEP104,
            );

            let psk = wpa.contains(NM80211ApSecurityFlags::KEY_MGMT_PSK);
            let eap = wpa.contains(NM80211ApSecurityFlags::KEY_MGMT_802_1X);

            return match (wep, psk, eap) {
                (false, false, false) => WifiSec::Open,
                (true, _, _) => WifiSec::Wep,
                (false, true, false) => WifiSec::Wpa1Psk,
                (false, false, true) => WifiSec::Wpa1Eap,
                (false, true, true) => WifiSec::Unknown, // WPA1 mixed
            };
        }

        // RSN present (WPA2/3/OWE).
        let wep = rsn.intersects(
            NM80211ApSecurityFlags::PAIR_WEP40
                | NM80211ApSecurityFlags::PAIR_WEP104
                | NM80211ApSecurityFlags::GROUP_WEP40
                | NM80211ApSecurityFlags::GROUP_WEP104,
        );

        if wep {
            return WifiSec::Wep;
        }

        if rsn.contains(NM80211ApSecurityFlags::KEY_MGMT_OWE_TM) {
            return WifiSec::OweTransition;
        }

        if rsn.contains(NM80211ApSecurityFlags::KEY_MGMT_OWE) {
            return WifiSec::Owe;
        }

        let psk = rsn.contains(NM80211ApSecurityFlags::KEY_MGMT_PSK);
        let sae = rsn.contains(NM80211ApSecurityFlags::KEY_MGMT_SAE);
        let eap = rsn.contains(NM80211ApSecurityFlags::KEY_MGMT_802_1X);

        match (sae, psk, eap) {
            (true, true, _) => WifiSec::Wpa2Wpa3Transitional,
            (true, false, _) => WifiSec::Wpa3Sae,
            (false, true, false) => WifiSec::Wpa2Psk,
            (false, false, true) => WifiSec::Enterprise,
            (false, false, false) => WifiSec::Open, // RSN IE but no auth bits
            _ => WifiSec::Unknown,
        }
    }

    pub fn parse(s: &str) -> Option<WifiSec> {
        match s.trim().to_lowercase().as_str() {
            "open" | "none" => Some(WifiSec::Open),
            "owe" => Some(WifiSec::Owe),
            "owetransition" => Some(WifiSec::OweTransition),
            "wep" => Some(WifiSec::Wep),
            "wpa1psk" => Some(WifiSec::Wpa1Psk),
            "wpa1eap" => Some(WifiSec::Wpa1Eap),
            "wpa2psk" | "wpa-psk" | "wpa" | "t:wpa" | "wpa2" => Some(WifiSec::Wpa2Psk),
            "wpa3sae" | "sae" | "wpa3" => Some(WifiSec::Wpa3Sae),
            "wpa2wpa3transitional" => Some(WifiSec::Wpa2Wpa3Transitional),
            "enterprise" | "wpa-eap" => Some(WifiSec::Enterprise),
            other => {
                // tolerate legacy/misconfigured strings like "sae wpa-psk" or "wpa-psk sae"
                let has_sae = other.split_whitespace().any(|t| t == "sae");
                let has_psk = other.split_whitespace().any(|t| t == "wpa-psk");
                match (has_psk, has_sae) {
                    (true, true) => Some(WifiSec::Wpa2Wpa3Transitional),
                    (true, false) => Some(WifiSec::Wpa2Psk),
                    (false, true) => Some(WifiSec::Wpa3Sae),
                    (false, false) => None,
                }
            }
        }
    }

    /// See https://networkmanager.dev/docs/api/1.52.0/settings-802-11-wireless-security.html
    pub fn as_nm_str(&self) -> &str {
        match self {
            WifiSec::Open => "none",
            WifiSec::Owe => "owe",
            WifiSec::OweTransition => "none",
            WifiSec::Wep => "none",
            WifiSec::Wpa1Psk => "wpa-psk",
            WifiSec::Wpa1Eap => "wpa-eap",
            WifiSec::Wpa2Psk => "wpa-psk",
            WifiSec::Wpa3Sae => "sae",
            WifiSec::Wpa2Wpa3Transitional => "wpa-psk",
            WifiSec::Enterprise => "wpa-eap",
            WifiSec::Unknown => "none",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::WifiSec;

    #[test]
    fn wifi_sec_can_parse_self_to_string() {
        for sec in [
            WifiSec::Open,
            WifiSec::Owe,
            WifiSec::OweTransition,
            WifiSec::Wep,
            WifiSec::Wpa1Psk,
            WifiSec::Wpa1Eap,
            WifiSec::Wpa2Psk,
            WifiSec::Wpa3Sae,
            WifiSec::Wpa2Wpa3Transitional,
            WifiSec::Enterprise,
        ] {
            let str = sec.to_string();
            let actual = WifiSec::parse(&str).unwrap();
            assert_eq!(actual, sec);
        }
    }
}
