use super::wifi;
use crate::network_manager::WifiSec;
use base64::Engine as _;
use chrono::{DateTime, Utc};
use color_eyre::{
    eyre::{bail, eyre, Context, ContextCompat},
    Result,
};
use orb_info::orb_os_release::OrbRelease;
use p256::{
    ecdsa::{signature::Verifier, Signature, VerifyingKey},
    pkcs8::DecodePublicKey,
};
use std::collections::HashMap;

#[derive(Debug, PartialEq, Clone)]
pub struct NetConfig {
    pub wifi_credentials: Option<wifi::Credentials>,
    pub wifi_enabled: Option<bool>,
    pub smart_switching: Option<bool>,
    pub airplane_mode: Option<bool>,
    pub created_at: DateTime<Utc>,
}

impl NetConfig {
    pub fn parse(netconfig_str: &str) -> Result<NetConfig> {
        let netconfig_str = netconfig_str.replace("WIFI:", "");
        let map: HashMap<_, _> = netconfig_str
            .split(";")
            .flat_map(|entry| entry.split_once(":"))
            .collect();

        let ver = map
            .get("NETCONFIG")
            .wrap_err("could not get netconfig ver")?;

        if *ver != "v1.0" {
            bail!("unspported netconfig ver: {ver}");
        }

        let get_bool = |field: &str| {
            map.get(field)
                .map(|f| f.parse())
                .transpose()
                .wrap_err_with(|| {
                    format!("could not parse {field}. netconfig: {netconfig_str}")
                })
        };

        let wifi_sec = map
            .get("T")
            .map(|sec| {
                WifiSec::parse(sec).wrap_err_with(|| format!("invalid wifi sec {sec}"))
            })
            .transpose()?;

        let ssid = map.get("S");
        let psk = map.get("P");
        let hidden = get_bool("H")?;

        let wifi_credentials =
            wifi_sec.zip(ssid).map(|(sec, ssid)| wifi::Credentials {
                ssid: ssid.to_string(),
                sec,
                psk: psk.map(|p| p.to_string()),
                hidden: hidden.unwrap_or_default(),
            });

        let wifi_enabled = get_bool("WIFI_ENABLED")?;
        let smart_switching = get_bool("FALLBACK")?;
        let airplane_mode = get_bool("AIRPLANE")?;

        let created_at = map
            .get("TS")
            .map(|x| x.parse())
            .transpose()
            .wrap_err_with(|| {
                format!("could not parse timestamp from netconfig: {netconfig_str}")
            })?
            .wrap_err_with(|| format!("TS missing from netconfig: {netconfig_str}"))?;

        let created_at = DateTime::from_timestamp(created_at, 0)
            .wrap_err_with(|| format!("{created_at} is not a valid timestamp"))?;

        Ok(Self {
            wifi_credentials,
            wifi_enabled,
            smart_switching,
            airplane_mode,
            created_at,
        })
    }

    // verifies the sig or netconfig qr with ECC_NIST_P256 pub key
    pub fn verify_signature(qr_contents: &str, release: OrbRelease) -> Result<()> {
        static PROD: &str = "-----BEGIN PUBLIC KEY-----
MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAE1Rwr5CEvtWzmcQu4IS+VFmkRiZdM
SmNKUZ+THL5nRV2kYmNRc6fBBFiam5HjYRlbFGKjctJZ3gXQz4Bv30+FOw==
-----END PUBLIC KEY-----";

        static STAGE: &str = "-----BEGIN PUBLIC KEY-----
MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEAfVD06rPhda6auRt3cK+Ntqrz5Fo
E5StFkWbhShXco5lwJPtZitWdElNxaCzMmJiyF6AXyd11SRzxE4FjUZp8Q==
-----END PUBLIC KEY-----";

        let pub_key = match release {
            OrbRelease::Dev => STAGE,
            _ => PROD,
        };

        let (msg, sig) = qr_contents.split_once("SIG:").wrap_err("SIG not found")?;

        let sig_der = base64::engine::general_purpose::STANDARD
            .decode(sig.trim_end_matches(['\r', '\n']))
            .wrap_err("bad base64 sig")?;

        let sig = Signature::from_der(&sig_der)?;
        let sig = match sig.normalize_s() {
            None => sig,
            Some(normalized) => normalized,
        };

        VerifyingKey::from_public_key_pem(pub_key)?
            .verify(msg.as_bytes(), &sig)
            .map_err(|e| eyre!("verification of qr sig failed: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        network_manager::WifiSec,
        service::{netconfig::NetConfig, wifi},
    };
    use chrono::DateTime;
    use orb_info::orb_os_release::OrbRelease;

    #[test]
    fn it_verifies_sig() {
        const VALID_STAGE: &str = "NETCONFIG:v1.0;WIFI_ENABLED:true;FALLBACK:false;AIRPLANE:false;WIFI:T:WPA;S:network;P:password;;TS:1758277671;SIG:MEYCIQD/HtYGcxwOdNUppjRaGKjSOTnSTI8zJIJH9iDagsT3tAIhAPPq6qgEMGzm6HkRQYpxp86nfDhvUYFrneS2vul4anPA";
        const INVALID_STAGE: &str = "NETCONFIG:v1.0;WIFI_ENABLED:false;FALLBACK:false;AIRPLANE:false;WIFI:T:WPA;S:network;P:password;;TS:1758277671;SIG:MEYCIQD/HtYGcxwOdNUppjRaGKjSOTnSTI8zJIJH9iDagsT3tAIhAPPq6qgEMGzm6HkRQYpxp86nfDhvUYFrneS2vul4anPA";

        let valid = NetConfig::verify_signature(VALID_STAGE, OrbRelease::Dev);
        let invalid = NetConfig::verify_signature(INVALID_STAGE, OrbRelease::Dev);

        assert!(valid.is_ok(), "{valid:?}");
        assert!(invalid.is_err(), "{invalid:?}");
    }

    #[test]
    fn it_parses_netconfig() {
        for (netconfig_str, expected) in [
            (
            "NETCONFIG:v1.0;WIFI_ENABLED:true;FALLBACK:false;AIRPLANE:false;WIFI:T:WPA;S:network;P:password;;TS:1758277671;", NetConfig {
                            wifi_credentials: Some(wifi::Credentials {
                                ssid: "network".to_string(),
                                sec: WifiSec::WpaPsk,
                                psk: Some("password".to_string()),
                                hidden: false,
                            }),
                            wifi_enabled: Some(true),
                            smart_switching: Some(false),
                            airplane_mode: Some(false),
                            created_at: DateTime::from_timestamp(1758277671, 0).unwrap(),
                        }),
            (
            "NETCONFIG:v1.0;WIFI_ENABLED:false;AIRPLANE:false;WIFI:T:WPA;S:network;TS:1758277671;", NetConfig {
                            wifi_credentials: Some(wifi::Credentials {
                                ssid: "network".to_string(),
                                sec: WifiSec::WpaPsk,
                                psk: None,
                                hidden: false,
                            }),
                            wifi_enabled: Some(false),
                            smart_switching: None,
                            airplane_mode: Some(false),
                            created_at: DateTime::from_timestamp(1758277671, 0).unwrap(),
                        }),
            (
            "NETCONFIG:v1.0;WIFI:T:WPA;S:network;P:password;;TS:1758277671;", NetConfig {
                            wifi_credentials: Some(wifi::Credentials {
                                ssid: "network".to_string(),
                                sec: WifiSec::WpaPsk,
                                psk: Some("password".to_string()),
                                hidden: false,
                            }),
                            wifi_enabled: None,
                            smart_switching: None,
                            airplane_mode: None,
                            created_at: DateTime::from_timestamp(1758277671, 0).unwrap(),
                        }),
            (
            "NETCONFIG:v1.0;WIFI_ENABLED:true;FALLBACK:true;AIRPLANE:true;TS:1758277671;", NetConfig {
                            wifi_credentials: None,
                            wifi_enabled: Some(true),
                            smart_switching: Some(true),
                            airplane_mode: Some(true),
                            created_at: DateTime::from_timestamp(1758277671, 0).unwrap(),
                        }),
        ] {
            let actual = NetConfig::parse(netconfig_str).unwrap();
            assert_eq!(actual, expected);
        }
    }
}
