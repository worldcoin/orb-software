//! Orb networking.

pub mod credentials;
mod wpa_dbus;

use self::{
    credentials::{AuthType, Credentials},
    wpa_dbus::{InterfaceProxySignalPoll, NetworkProxyExtractedProps},
};
// use crate::logger::{LogOnError, DATADOG, NO_TAGS};
use data_encoding::HEXLOWER;
use eyre::{eyre, OptionExt, Result, WrapErr};
use futures::StreamExt;
use ring::{pbkdf2, pbkdf2::PBKDF2_HMAC_SHA1};
use std::{borrow::Cow, collections::HashMap, num::NonZeroU32, str};
use tracing::warn;

/// Network connection status.
#[derive(Debug, Clone, Copy)]
pub enum InterfaceStatus {
    /// WiFi is connected.
    Connected,
    /// WiFi is disconnected.
    Disconnected,
    /// Connection is in progress.
    InProgress,
}

/// Gets the status of the `iface_name` network interface.
///
/// # Example
/// ```no_run
/// # tokio_test::block_on(async {
/// let status = orb_wpa_supplicant::iface_status("wlan0").await.unwrap();
/// println!("{status:?}");
/// # })
/// ```
pub async fn iface_status(iface_name: &str) -> Result<InterfaceStatus> {
    let sys_conn = sys_conn().await?;
    let proxy = wpa_dbus::GeneralProxy::new(sys_conn)
        .await
        .wrap_err("failed to create `fi.w1.wpa_supplicant1 (General)` dbus proxy")?;

    let iface = get_wifi_interface(sys_conn, proxy, iface_name).await?;
    match iface
        .state()
        .await
        .wrap_err("failed to get `state` property on interface proxy")?
        .as_str()
    {
        "completed" => Ok(InterfaceStatus::Connected),
        "disconnected" | "inactive" | "interface_disabled" | "scanning" => {
            Ok(InterfaceStatus::Disconnected)
        }
        "authenticating" | "associating" | "associated" | "4way_handshake"
        | "group_handshake" | "unknown" => Ok(InterfaceStatus::InProgress),
        unknown_value => {
            warn!(
                "Unexpected string returned from wpa_supplicant state, assuming wifi connection \
                 in progress: {unknown_value}"
            );
            Ok(InterfaceStatus::InProgress)
        }
    }
}

/// Gets the rssi for the current wifi network. Will return an error if there is no
/// current network.
/// # Example
/// ```no_run
/// # tokio_test::block_on(async {
/// let rssi = orb_wpa_supplicant::current_network_rssi("wlan0").await.unwrap();
/// println!("{rssi}");
/// # })
/// ```
pub async fn current_network_rssi(iface_name: &str) -> Result<i32> {
    let conn = sys_conn().await?;
    let proxy = wpa_dbus::GeneralProxy::new(conn)
        .await
        .wrap_err("failed to create `fi.w1.wpa_supplicant1 (General)` dbus proxy")?;

    let iface = get_wifi_interface(conn, proxy, iface_name).await?;

    let signal_poll_extracts = {
        let val = iface
            .signal_poll()
            .await
            .wrap_err("error while calling signal_poll")?;
        let hmap = HashMap::try_from(val)
            .wrap_err("conversion to hashmap was thought to be infallible")?;
        InterfaceProxySignalPoll::from_dbus(hmap)
            .wrap_err("conversion to struct was thought to be infallible")?
    };
    signal_poll_extracts.rssi.ok_or_eyre("no rssi found")
}

/// Gets the SSID of the current wifi network.
///
/// # Example
/// ```no_run
/// # tokio_test::block_on(async {
/// let ssid = orb_wpa_supplicant::current_network_ssid("wlan0").await.unwrap();
/// println!("{ssid}");
/// # })
/// ```
pub async fn current_network_ssid(iface_name: &str) -> Result<String> {
    let conn = sys_conn().await?;
    let proxy = wpa_dbus::GeneralProxy::new(conn)
        .await
        .wrap_err("failed to create `fi.w1.wpa_supplicant1 (General)` dbus proxy")?;

    let iface = get_wifi_interface(conn, proxy, iface_name).await?;
    let bss_path = iface.current_bss().await?;
    let bss_proxy = wpa_dbus::BSSProxy::builder(conn)
        .path(bss_path)?
        .build()
        .await?;
    let ssid = bss_proxy
        .ssid()
        .await
        .wrap_err("failed to get ssid from bss proxy")?;
    let cow = String::from_utf8_lossy(&ssid);
    if let Cow::Owned(_) = cow {
        tracing::warn!("network contained non-utf8 characters, stripping...");
    }
    Ok(cow.to_string())
}

/// Joins WiFi network using the given `credentials`.
pub async fn join(iface_name: &str, credentials: Credentials) -> Result<()> {
    let conn = sys_conn().await?;
    let proxy = wpa_dbus::GeneralProxy::new(conn)
        .await
        .wrap_err("failed to create `fi.w1.wpa_supplicant1 (General)` dbus proxy")?;

    let iface = get_wifi_interface(conn, proxy, iface_name).await?;

    let check_for_matching_ssid = || async {
        get_best_matching_bss(conn, &iface, &credentials.ssid)
            .await
            .wrap_err("Failed to search scan results")
            .map(|bss| bss.is_some())
    };

    // Scan/check that the network exists
    if !check_for_matching_ssid().await? {
        let mut signal_scan_done = iface.receive_scan_done().await.wrap_err(
            "failed to register `fi.w1.wpa_supplicant1.Interface.ScanDone` signal listener",
        )?;

        iface
            .scan(HashMap::from([
                ("Type", "active".into()),
                ("SSIDs", vec![&credentials.ssid].into()),
            ]))
            .await
            .wrap_err("failed initiating BSS scan")?;

        tokio::time::timeout(
            tokio::time::Duration::from_secs(5),
            signal_scan_done.next(),
        )
        .await
        .wrap_err("scan timed out")
        // Even if we timeout or there is a dbus error, we should still check if an
        // unfinished scan found a network that matches our criteria, so we don't
        // bubble the error up.
        .map_err(|err| tracing::warn!("error occurred waiting for AP scan: {err:?}"))
        .ok();

        if check_for_matching_ssid().await? {
            return Err(eyre!("Failed to find matching SSID even after active scan"))?;
        }
    }

    let (net_path, _net) = find_or_add_network(conn, &iface, &credentials).await?;

    let future_timeout =
        std::pin::pin!(tokio::time::sleep(tokio::time::Duration::from_secs(5)));
    let mut signal_state_changed = iface
        .receive_state_changed()
        .await
        .take_until(future_timeout);

    iface
        .select_network(net_path)
        .await
        .wrap_err("failed to select network")?;

    while let Some(value) = signal_state_changed.next().await {
        let state = value
            .get()
            .await
            .map_err(|err| {
                tracing::error!(
                    "failed to get state value from state property changed signal: `{err:?}`"
                )
            })
            .ok();
        match state.as_deref() {
            Some("completed" | "disconnected" | "inactive" | "unknown") => {
                tracing::debug!("connection finished (`{state:?}`)");
                break;
            }
            Some(
                "scanning" | "authenticating" | "associating" | "associated"
                | "4way_handshake" | "group_handshake" | _,
            ) => {
                tracing::debug!("still waiting for connection (`{state:?}`)...");
            }
            None => {}
        };
    }

    Ok(())
}

static SYS_CONN: tokio::sync::OnceCell<zbus::Connection> =
    tokio::sync::OnceCell::const_new();
async fn sys_conn() -> Result<&'static zbus::Connection> {
    SYS_CONN
        .get_or_try_init(|| async {
            zbus::Connection::system()
                .await
                .wrap_err("failed to connect to system dbus")
        })
        .await
}

/// `iface_name` example: "wlan0"
async fn get_wifi_interface<'a>(
    conn: &zbus::Connection,
    proxy: wpa_dbus::GeneralProxy<'a>,
    iface_name: &str,
) -> Result<wpa_dbus::InterfaceProxy<'a>> {
    let iface_path = proxy
        .get_interface(iface_name)
        .await
        .wrap_err_with(|| format!("failed getting `{iface_name}` interface path"))?;
    wpa_dbus::InterfaceProxy::builder(conn)
        .path(iface_path.clone())
        .wrap_err_with(|| format!("failed setting iface proxy path `{iface_path:?}`"))?
        .build()
        .await
        .wrap_err("failed to create `fi.w1.wpa_supplicant1.Interface` dbus proxy")
}

async fn get_best_matching_bss<'a>(
    conn: &zbus::Connection,
    iface_proxy: &wpa_dbus::InterfaceProxy<'a>,
    target_ssid: &str,
) -> Result<Option<wpa_dbus::BSSProxy<'a>>> {
    let bss_list: Vec<wpa_dbus::BSSProxy<'_>> = get_bss_list(conn, iface_proxy).await?;
    let mut target_bss: Option<wpa_dbus::BSSProxy> = None;
    for bss in bss_list.into_iter() {
        let ssid = String::from_utf8(
            bss.ssid()
                .await
                .wrap_err("failed to get `ssid` property on bss proxy")?,
        )
        .wrap_err("failed to convert ssid byte array to UTF-8 string")?;
        if target_ssid != ssid.as_str() {
            continue;
        }
        let rssi = bss
            .signal()
            .await
            .wrap_err("failed to get `signal` property on bss proxy")?;
        if target_bss.is_none()
            || rssi
                > target_bss
                    .as_ref()
                    .unwrap()
                    .signal()
                    .await
                    .wrap_err("failed to get `signal` property on bss proxy")?
        {
            target_bss = Some(bss)
        }
    }
    Ok(target_bss)
}

async fn get_bss_list<'a>(
    conn: &zbus::Connection,
    iface_proxy: &wpa_dbus::InterfaceProxy<'a>,
) -> Result<Vec<wpa_dbus::BSSProxy<'a>>> {
    let bss_list: Vec<zbus::zvariant::OwnedObjectPath> = iface_proxy
        .bsss()
        .await
        .wrap_err("failed to get `bsss` property on interface proxy")?;

    // We log the errors in the filter_map step
    Ok(futures::stream::iter(bss_list)
        .map(|path| async {
            wpa_dbus::BSSProxy::builder(conn)
                .path(path)
                .wrap_err("failed setting bss proxy path `{path:?}`")?
                .build()
                .await
                .wrap_err("failed to create `fi.w1.wpa_supplicant1.BSS` dbus proxy")
        })
        .filter_map(|res| async move {
            res.await
                .map_err(|err| {
                    tracing::warn!(
                        "issue converting BSS object path to BSS instance: `{err:?}`"
                    )
                })
                .ok()
        })
        .collect::<Vec<wpa_dbus::BSSProxy>>()
        .await)
}

/// Find or add a network to the wpa_supplicant daemon
///
/// # Known Issues
///
/// - *You cannot add a PBKDF2-derived PSK over DBus...*
///   The `AddNetwork` DBus method takes args as a kv-map which matches the
///   wpa_supplicant.conf(5). Within that conf, a PSK can either be represented as a
///   quoted plaintext string, or as an unquoted 64-character hex representation of the
///   PBKDF2-derived key.
///  
///   When wpa_supplicant handles `AddNetwork`'s network kv-map argument, the values get
///   parsed into the `wpa_ssid` struct based on the DBus value type. In the case of
///   strings, they get filtered through the `should_quote_opt` method which simply
///   looks up the field name in a `dont_quote` map.
///  
///   The PSK is missing from this map, and so our password is always quoted, meaning we
///   are only able to configure a network with plaintext passwords.
///
/// - *We cannot check that an existing `Network` instance matches our `Credentials`*
///   There is no immediate way to get the PSK (PBKDF2-derived or plaintext) from the
///   `Network`s properties.
///
/// - *Occasionally, converting from a `Network` owned object path to an actual
///   `Network` interface fails*.
async fn find_or_add_network<'a>(
    conn: &zbus::Connection,
    iface_proxy: &wpa_dbus::InterfaceProxy<'a>,
    credentials: &Credentials,
) -> Result<(zbus::zvariant::OwnedObjectPath, wpa_dbus::NetworkProxy<'a>)> {
    let network_list: Vec<zbus::zvariant::OwnedObjectPath> = iface_proxy
        .networks()
        .await
        .wrap_err("failed to get `networks` property on interface proxy")?;

    async fn find_network<'a>(
        conn: &zbus::Connection,
        network_list: Vec<zbus::zvariant::OwnedObjectPath>,
        compare_to_creds: &Credentials,
    ) -> Result<Option<(zbus::zvariant::OwnedObjectPath, wpa_dbus::NetworkProxy<'a>)>>
    {
        for net_path in network_list {
            let net_proxy = wpa_dbus::NetworkProxy::builder(conn)
                .path(net_path.clone())
                .wrap_err_with(|| {
                    format!("failed setting network proxy path `{net_path:?}`")
                })?
                .build()
                .await
                .wrap_err(
                    "failed to create `fi.w1.wpa_supplicant1.Network` dbus proxy",
                )?;
            let props = net_proxy
                .properties()
                .await
                .wrap_err("failed to get `properties` property on network proxy")?;
            let extracted_props = NetworkProxyExtractedProps::from_dbus(props)
                .wrap_err("Failed to extract properties from dbus")?;
            if extracted_props.matches(compare_to_creds) {
                return Ok(Some((net_path, net_proxy)));
            }
        }

        Ok(None)
    }

    if let Some(network) = find_network(conn, network_list, credentials).await? {
        return Ok(network);
    }

    iface_proxy
        .remove_all_networks()
        .await
        .wrap_err("failed to remove all networks from interface proxy")?;

    let network_properties = {
        let mut map = HashMap::<&str, zbus::zvariant::Value<'_>>::new();
        map.insert("ssid", credentials.ssid.as_str().into());
        if credentials.password.is_some() {
            let psk: &str = &credentials.password.as_ref().unwrap().0;
            map.insert("psk", psk.into());
        }
        match credentials.auth_type {
            AuthType::Wep => {
                map.insert("key_mgmt", "NONE".into());
            }
            AuthType::Wpa | AuthType::Sae => {
                map.insert("key_mgmt", "WPA-PSK".into());
            }
            AuthType::Nopass => {}
        }
        if credentials.hidden {
            map.insert("scan_ssid", 1.into());
        }
        map
    };
    iface_proxy
        .add_network(network_properties)
        .await
        .map(|path| async move {
            Ok((
                path.clone(),
                wpa_dbus::NetworkProxy::builder(conn)
                    .path(path)
                    .wrap_err("failed setting network proxy path `{path:?}`")?
                    .build()
                    .await
                    .wrap_err(
                        "failed to create `fi.w1.wpa_supplicant1.Network` dbus proxy",
                    )?,
            ))
        })?
        .await
}

// Using hex string encoding, because `wpa_supplicant.conf` string escaping
// schema is not well-defined.
fn hex_string<T: AsRef<[u8]>>(input: T) -> String {
    HEXLOWER.encode(input.as_ref())
}

fn wpa_passphrase(ssid: &str, passphrase: &str) -> String {
    let mut hash = [0_u8; 32];
    pbkdf2::derive(
        PBKDF2_HMAC_SHA1,
        NonZeroU32::new(4096).unwrap(),
        ssid.as_bytes(),
        passphrase.as_bytes(),
        &mut hash,
    );
    hex_string(hash)
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[test]
    fn test_hex_string() {
        assert_eq!(hex_string(b"worldcoin"), "776f726c64636f696e");
        assert_eq!(hex_string(b"\0"), "00");
    }

    #[test]
    fn test_wpa_passphrase() {
        assert_eq!(
            wpa_passphrase("worldcoin", "12345678"),
            "5c1f986129b5a10564a66899f10a2989d4deb8f9a9ba504c68e535d7a3c8e5ba"
        );
    }
}
