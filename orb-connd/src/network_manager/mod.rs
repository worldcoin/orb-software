use bon::bon;
use color_eyre::{
    eyre::{bail, ContextCompat},
    Result,
};
use rusty_network_manager::{
    AccessPointProxy, ActiveProxy, DeviceProxy, NetworkManagerProxy,
    SettingsConnectionProxy, SettingsProxy,
};
use std::collections::HashMap;
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

    /// Adds a wifi profile ensure id uniqueness
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
    ) -> Result<OwnedObjectPath> {
        self.remove_profile(id).await?;

        let connection = HashMap::from_iter([
            kv("id", id),
            kv("type", "802-11-wireless"),
            kv("autoconnect", autoconnect),
            kv("autoconnect-priority", priority),
        ]);

        let wifi = HashMap::from_iter([
            kv("ssid", ssid.as_bytes()),
            kv("mode", "infrastructure"),
            kv("hidden", hidden),
        ]);

        let sec = HashMap::from_iter([kv("key-mgmt", sec.as_str()), kv("psk", psk)]);

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

    /// Adds a cellular profile ensure id uniqueness
    #[builder(finish_fn=add)]
    pub async fn cellular_profile(
        &self,
        #[builder(start_fn)] id: &str,
        apn: &str,
        iface: &str,
        #[builder(default = 0)] priority: i32,
    ) -> Result<()> {
        self.remove_profile(id).await?;

        let connection = HashMap::from_iter([
            kv("id", id),
            kv("type", "gsm"),
            kv("interface-name", iface),
            kv("autoconnect", true),
            kv("autoconnect-priority", priority),
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WifiSec {
    /// WPA2 or WPA3 personal
    WpaPsk,
    /// WPA3 only
    Wpa3Sae,
}

impl WifiSec {
    pub fn parse(s: &str) -> Option<WifiSec> {
        match s.trim().to_lowercase().as_str() {
            "sae" => Some(WifiSec::Wpa3Sae),
            "wpa-psk" | "wpa" | "t:wpa" => Some(WifiSec::WpaPsk),
            other => {
                // tolerate legacy/misconfigured strings like "sae wpa-psk" or "wpa-psk sae"
                let has_sae = other.split_whitespace().any(|t| t == "sae");
                let has_psk = other.split_whitespace().any(|t| t == "wpa-psk");
                if has_sae && has_psk {
                    Some(WifiSec::WpaPsk)
                } else {
                    None
                }
            }
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            WifiSec::WpaPsk => "wpa-psk",
            WifiSec::Wpa3Sae => "sae",
        }
    }
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
