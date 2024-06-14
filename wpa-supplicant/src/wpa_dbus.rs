//! See <https://w1.fi/wpa_supplicant/devel/dbus.html>
//!
//! To mess around with the `wpa_supplicant1` DBus API, you can try the
//! following commands:
//!
//! ```shell
//! $ busctl introspect \
//!     fi.w1.wpa_supplicant1 \
//!     /fi/w1/wpa_supplicant1 \
//!     fi.w1.wpa_supplicant1
//! $ busctl call \
//!     fi.w1.wpa_supplicant1 \
//!     /fi/w1/wpa_supplicant1 \
//!     fi.w1.wpa_supplicant1 GetInterface s wlan0
//! $ busctl call \
//!     fi.w1.wpa_supplicant1 \
//!     /fi/w1/wpa_supplicant1 \
//!     fi.w1.wpa_supplicant1 \
//!     CreateInterface a{sv} 1 Ifname s wlan0
//! $ busctl introspect \
//!     fi.w1.wpa_supplicant1 \
//!     /fi/w1/wpa_supplicant1/Interfaces/0 \
//!     fi.w1.wpa_supplicant1.Interface
//! $ busctl call
//!     fi.w1.wpa_supplicant1 \
//!     /fi/w1/wpa_supplicant1/Interfaces/0 \
//!     org.freedesktop.DBus.Properties \
//!     Get ss 'fi.w1.wpa_supplicant1.Interface' 'State'
//! $ busctl introspect \
//!     fi.w1.wpa_supplicant1 \
//!     /fi/w1/wpa_supplicant1/Interfaces/0/Networks/0 \
//!     fi.w1.wpa_supplicant1.Network
//! ```
use eyre::{OptionExt, Result, WrapErr};
use std::collections::HashMap;
use zbus::zvariant::OwnedValue as ZbusOwnedValue;

use crate::{
    credentials::{AuthType, Credentials},
    wpa_passphrase,
};

#[zbus::proxy(
    interface = "fi.w1.wpa_supplicant1",
    default_service = "fi.w1.wpa_supplicant1",
    default_path = "/fi/w1/wpa_supplicant1"
)]
pub trait General {
    fn get_interface(
        &self,
        name: &str,
    ) -> zbus::Result<zbus::zvariant::OwnedObjectPath>;
    fn create_interface(
        &self,
        args: HashMap<&str, zbus::zvariant::Value<'_>>,
    ) -> zbus::Result<zbus::zvariant::OwnedObjectPath>;

    #[zbus(property)]
    pub fn interfaces(&self) -> zbus::Result<Vec<zbus::zvariant::OwnedObjectPath>>;
}

#[zbus::proxy(
    interface = "fi.w1.wpa_supplicant1.Interface",
    default_service = "fi.w1.wpa_supplicant1"
)]
pub trait Interface {
    /// Scans for BSSs using map-named arguments
    ///
    /// # Args
    /// - "Type" (_string_ / _s_, *required*)
    ///     - "active" or "passive"
    /// - "SSIDs" (_array of array of bytes_ / _aay_, _optional_)
    ///     - Each element of the array is an SSID string represented as a byte array
    fn scan(&self, args: HashMap<&str, zbus::zvariant::Value<'_>>) -> zbus::Result<()>;
    fn disconnect(&self) -> zbus::Result<()>;
    fn add_network(
        &self,
        args: HashMap<&str, zbus::zvariant::Value<'_>>,
    ) -> zbus::Result<zbus::zvariant::OwnedObjectPath>;
    fn remove_network(
        &self,
        network: zbus::zvariant::OwnedObjectPath,
    ) -> zbus::Result<()>;
    fn remove_all_networks(&self) -> zbus::Result<()>;
    fn select_network(
        &self,
        network: zbus::zvariant::OwnedObjectPath,
    ) -> zbus::Result<()>;

    /// See [`InterfaceProxySignalPoll`]
    fn signal_poll(&self) -> zbus::Result<zbus::zvariant::OwnedValue>;

    #[zbus(name = "FlushBSS")]
    fn flush_bss(&self, age: u32) -> zbus::Result<()>;

    #[zbus(property)]
    fn state(&self) -> zbus::Result<String>;

    #[zbus(property)]
    fn current_network(&self) -> zbus::Result<zbus::zvariant::OwnedObjectPath>;

    #[zbus(property, name = "CurrentBSS")]
    fn current_bss(&self) -> zbus::Result<zbus::zvariant::OwnedObjectPath>;

    #[zbus(property)]
    fn config_file(&self) -> zbus::Result<String>;

    #[zbus(property)]
    fn disconnect_reason(&self) -> zbus::Result<i32>;

    #[zbus(property)]
    fn networks(&self) -> zbus::Result<Vec<zbus::zvariant::OwnedObjectPath>>;

    /// Get list of BSSs from scan results
    #[zbus(property, name = "BSSs")]
    fn bsss(&self) -> zbus::Result<Vec<zbus::zvariant::OwnedObjectPath>>;

    #[zbus(property, name = "BSSExpireAge")]
    fn bss_expire_age(&self) -> zbus::Result<u32>;
    #[zbus(property, name = "BSSExpireAge")]
    fn set_bss_expire_age(&self, value: u32) -> zbus::Result<()>;

    #[zbus(property, name = "BSSExpireCount")]
    fn bss_expire_count(&self) -> zbus::Result<u32>;
    #[zbus(property, name = "BSSExpireCount")]
    fn set_bss_expire_count(&self, value: u32) -> zbus::Result<()>;

    #[zbus(signal)]
    fn scan_done(&self, success: bool) -> zbus::Result<()>;

    #[zbus(signal)]
    fn properties_changed(
        &self,
        properties: HashMap<&str, zbus::zvariant::Value<'_>>,
    ) -> zbus::Result<()>;
}

/// Represents a BSS (Basic Service Set)
#[zbus::proxy(
    interface = "fi.w1.wpa_supplicant1.BSS",
    default_service = "fi.w1.wpa_supplicant1"
)]
pub trait BSS {
    #[zbus(property, name = "Signal")]
    fn signal(&self) -> zbus::Result<i16>;

    #[zbus(property, name = "SSID")]
    fn ssid(&self) -> zbus::Result<Vec<u8>>;

    #[zbus(property, name = "BSSID")]
    fn bssid(&self) -> zbus::Result<Vec<u8>>;

    #[zbus(property, name = "WPA")]
    fn wpa(&self) -> zbus::Result<HashMap<String, ZbusOwnedValue>>;
}

/// Network interface wrapping a map which represents a wpa_supplicant.conf(5)
/// "network" block.
///
/// # Basic Properties
/// - ssid (_required_)
/// - scan_ssid
/// - priority
/// - psk
///
/// For more, see _man wpa_supplicant.conf(5)_
#[zbus::proxy(
    interface = "fi.w1.wpa_supplicant1.Network",
    default_service = "fi.w1.wpa_supplicant1"
)]
pub trait Network {
    #[zbus(property, name = "Properties")]
    fn properties(&self) -> zbus::Result<HashMap<String, ZbusOwnedValue>>;

    #[zbus(property, name = "Enabled")]
    fn enabled(&self) -> zbus::Result<bool>;
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct InterfaceProxySignalPoll {
    /// Link speed in Mbps. Optional.
    pub linkspeed: Option<i32>,
    /// Noise level in dBm. Optional.
    pub noise: Option<i32>,
    /// Channel width. Optional.
    pub width: Option<String>,
    /// Frequency in MHz. Optional.
    pub frequency: Option<u32>,
    /// RSSI in dBm. Optional.
    pub rssi: Option<i32>,
    /// Average RSSI in dBm. Optional.
    pub avg_rssi: Option<i32>,
    /// VHT segment 1 frequency in MHz. Optional.
    pub center_frq1: Option<i32>,
    /// VHT segment 2 frequency in MHz. Optional.
    pub center_frq2: Option<i32>,
}

impl InterfaceProxySignalPoll {
    pub fn from_dbus(dbus: HashMap<String, ZbusOwnedValue>) -> Result<Self> {
        Ok(Self {
            linkspeed: extract_prop(&dbus, "linkspeed")?,
            noise: extract_prop(&dbus, "noise")?,
            width: extract_prop(&dbus, "width")?,
            frequency: extract_prop(&dbus, "frequency")?,
            rssi: extract_prop(&dbus, "rssi")?,
            avg_rssi: extract_prop(&dbus, "avg-rssi")?,
            center_frq1: extract_prop(&dbus, "center-frq1")?,
            center_frq2: extract_prop(&dbus, "center-frq2")?,
        })
    }
}

/// The wifi properties extracted from zbus
#[derive(Debug)]
pub struct NetworkProxyExtractedProps {
    ssid: String,
    psk: Option<String>,
    // won't exist if there is no password
    key_mgmt: Option<String>,
}

impl NetworkProxyExtractedProps {
    pub fn from_dbus(dbus: HashMap<String, ZbusOwnedValue>) -> Result<Self> {
        let ssid =
            extract_prop(&dbus, "ssid")?.ok_or_eyre("Expected SSID to be present")?;
        let password = extract_prop(&dbus, "psk")?;
        let key_mgmt = extract_prop(&dbus, "key_mgmt")?;

        Ok(Self {
            ssid,
            psk: password,
            key_mgmt,
        })
    }

    /// Note: We return false if the ssid was given to us by dbus as unquoted
    pub fn matches(&self, creds: &Credentials) -> bool {
        let quoted_creds_ssid = format!("\"{}\"", &creds.ssid);
        if self.ssid != quoted_creds_ssid {
            return false;
        }
        if self.psk
            != creds
                .password
                .as_ref()
                .map(|p| wpa_passphrase(&creds.ssid, p))
        {
            return false;
        }
        if creds.password.is_none() {
            assert_eq!(creds.auth_type, AuthType::Nopass);
        }
        match (self.key_mgmt.as_deref(), creds.auth_type) {
            (None, AuthType::Nopass) => true,
            (Some("NONE"), AuthType::Wep) => true,
            (Some("WPA-PSK"), AuthType::Wpa | AuthType::Sae) => true,
            (Some("WPA-PSK" | "NONE"), _) => false,
            (Some(km), at) => {
                tracing::warn!(
                    "Unknown auth type encountered! Assuming networks dont match.
                    key_mgmt: {km:?}, auth_type: {at:?}"
                );
                false
            }
            _ => false,
        }
    }
}

/// Extract a property named `prop_name` of type `T`.
/// Returns `Ok(None)` if the property doesn't exist, or `Err` if the conversion failed.
fn extract_prop<T>(
    dbus: &HashMap<String, ZbusOwnedValue>,
    prop_name: &str,
) -> Result<Option<T>>
where
    T: TryFrom<ZbusOwnedValue>,
    // basically, TryFrom must return a StdError
    <T as TryFrom<ZbusOwnedValue>>::Error: std::error::Error + Send + Sync + 'static,
{
    let Some(prop) = dbus.get(prop_name) else {
        return Ok(None);
    };
    T::try_from(prop.try_clone().wrap_err("failed to clone property")?)
        .wrap_err_with(|| {
            format!(
                "Failed to convert property (name: {prop_name}, val: {prop:?}) to {}",
                std::any::type_name::<T>()
            )
        })
        .map(|v| Some(v))
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_extract_prop() {
        let prop_value =
            ZbusOwnedValue::try_from(zbus::zvariant::Value::Str("1337".into()))
                .unwrap();
        let prop_name = "prop_name";
        let map = HashMap::from([(prop_name.to_owned(), prop_value)]);

        // Check proper conversion and present property
        assert_eq!(
            extract_prop::<String>(&map, prop_name).expect("conversion failed"),
            Some(String::from("1337"))
        );
        // check proper conversion but absent property
        assert!(matches!(
            extract_prop::<String>(&map, "does_not_exist"),
            Ok(None)
        ));
        // check improper conversion
        assert!(extract_prop::<i32>(&map, prop_name).is_err());
    }
}
