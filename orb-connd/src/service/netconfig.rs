use super::{
    mecard::{parse_bool, parse_field, parse_string},
    wifi,
};
use crate::service::{
    mecard,
    wifi::{parse_hidden, parse_password, parse_ssid, Auth, Password},
};
use base64::Engine as _;
use chrono::{DateTime, Utc};
use color_eyre::{
    eyre::{eyre, Context, ContextCompat},
    Result,
};
use nom::{bytes::complete::tag, IResult};
use orb_info::orb_os_release::OrbRelease;
use p256::{
    ecdsa::{signature::Verifier, Signature, VerifyingKey},
    pkcs8::DecodePublicKey,
};

#[derive(Debug, PartialEq, Clone)]
pub struct NetConfig {
    pub wifi_credentials: Option<wifi::Credentials>,
    pub wifi_enabled: Option<bool>,
    pub smart_switching: Option<bool>,
    pub airplane_mode: Option<bool>,
    pub created_at: DateTime<Utc>,
}

impl NetConfig {
    pub fn parse(input: &str) -> Result<NetConfig> {
        let (mut input, _) = tag::<_, _, ()>("NETCONFIG:v1.0;")(input)?;

        mecard::parse_fields! { input;
            Auth::parse => auth_type,
            parse_ssid => ssid,
            parse_password => password,
            parse_hidden => hidden,
            parse_wifi_enabled => wifi_enabled,
            parse_airplane_mode => airplane_mode,
            parse_smart_switching => smart_switching,
            parse_ts => created_at,
            parse_wifi_marker => _wifi_marker,
        };

        let created_at = created_at
            .wrap_err("timestamp missing from netconfig")?
            .parse()
            .wrap_err("failed to parse timestamp in netconfig")?;

        let created_at = DateTime::from_timestamp(created_at, 0)
            .wrap_err_with(|| format!("{created_at} is not a valid timestamp"))?;

        let wifi_credentials = ssid.filter(|ssid| !ssid.is_empty()).map(|ssid| {
            let psk = password.filter(|pwd| !pwd.is_empty()).map(Password);

            let auth = auth_type.unwrap_or_else(|| {
                if psk.is_some() {
                    Auth::Wpa
                } else {
                    Auth::Nopass
                }
            });

            wifi::Credentials {
                auth,
                ssid,
                psk,
                hidden: hidden.unwrap_or_default(),
            }
        });

        Ok(NetConfig {
            wifi_credentials,
            wifi_enabled,
            airplane_mode,
            smart_switching,
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

fn parse_wifi_enabled(input: &str) -> IResult<&str, bool> {
    parse_field(input, "WIFI_ENABLED", parse_bool)
}

fn parse_smart_switching(input: &str) -> IResult<&str, bool> {
    parse_field(input, "FALLBACK", parse_bool)
}

fn parse_airplane_mode(input: &str) -> IResult<&str, bool> {
    parse_field(input, "AIRPLANE", parse_bool)
}

fn parse_ts(input: &str) -> IResult<&str, String> {
    parse_field(input, "TS", parse_string)
}

fn parse_wifi_marker(input: &str) -> IResult<&str, ()> {
    let (input, _) = tag("WIFI:")(input)?;
    Ok((input, ()))
}

#[cfg(test)]
mod tests {
    use crate::service::{
        netconfig::NetConfig,
        wifi::{self, Auth},
    };
    use chrono::DateTime;
    use orb_info::orb_os_release::OrbRelease;

    #[test]
    fn it_verifies_sig() {
        const VALID_STAGE: &str = "NETCONFIG:v1.0;WIFI_ENABLED:true;FALLBACK:false;AIRPLANE:false;WIFI:T:WPA;S:network;P:password;;TS:1758277671;SIG:MEYCIQD/HtYGcxwOdNUppjRaGKjSOTnSTI8zJIJH9iDagsT3tAIhAPPq6qgEMGzm6HkRQYpxp86nfDhvUYFrneS2vul4anPA";
        const INVALID_STAGE: &str = "NETCONFIG:v1.0;WIFI_ENABLED:false;FALLBACK:false;AIRPLANE:false;WIFI:T:WPA;S:network;P:password;;TS:1758277671;SIG:MEYCIQD/HtYGcxwOdNUppjRaGKjSOTnSTI8zJIJH9iDagsT3tAIhAPPq6qgEMGzm6HkRQYpxp86nfDhvUYFrneS2vul4anPA";
        const VALID_STAGE_2: &str = "NETCONFIG:v1.0;WIFI_ENABLED:true;FALLBACK:false;AIRPLANE:false;T:WPA;S:network;P:password;TS:1759755209;SIG:MEQCIGBBSntcwyDWeuea4R08VRhx9bHviQkC3arNtCiMjUFrAiAzUVrLgOsHqp16p9u2xXPpzcIk0G7d8+GCwRjnKplJWA==";
        const VALID_PROD: &str = "NETCONFIG:v1.0;WIFI_ENABLED:true;FALLBACK:false;AIRPLANE:false;T:WPA;S:network;P:password;TS:1759755209;SIG:MEQCIDJjgj5QHJE+tOaekdcSrsWinaocltIoMRKk+wDc0i8MAiBvfHZzsbHxuyuLDxnL8Hbnh8qidxflannoPf3TQm+zgA==";

        let valid_stage = NetConfig::verify_signature(VALID_STAGE, OrbRelease::Dev);
        let invalid_stage = NetConfig::verify_signature(INVALID_STAGE, OrbRelease::Dev);
        let valid_stage_2 = NetConfig::verify_signature(VALID_STAGE_2, OrbRelease::Dev);
        let valid_prod = NetConfig::verify_signature(VALID_PROD, OrbRelease::Prod);

        assert!(valid_stage.is_ok(), "{valid_stage:?}");
        assert!(invalid_stage.is_err(), "{invalid_stage:?}");
        assert!(valid_stage_2.is_ok(), "{valid_stage_2:?}");
        assert!(valid_prod.is_ok(), "{valid_prod:?}");
    }

    #[test]
    fn it_parses_netconfig() {
        for (netconfig_str, expected) in [
            (
            "NETCONFIG:v1.0;WIFI_ENABLED:true;FALLBACK:false;AIRPLANE:false;T:WPA;S:network;P:password;TS:1758277671;", NetConfig {
                            wifi_credentials: Some(wifi::Credentials {
                                ssid: "network".to_string(),
                                auth: Auth::Wpa,
                                psk: Some("password".into()),
                                hidden: false,
                            }),
                            wifi_enabled: Some(true),
                            smart_switching: Some(false),
                            airplane_mode: Some(false),
                            created_at: DateTime::from_timestamp(1758277671, 0).unwrap(),
                        }),
            (
            "NETCONFIG:v1.0;T:WPA;WIFI_ENABLED:true;FALLBACK:false;AIRPLANE:false;S:network;P:password;;TS:1758277671;", NetConfig {
                            wifi_credentials: Some(wifi::Credentials {
                                ssid: "network".to_string(),
                                auth: Auth::Wpa,
                                psk: Some("password".into()),
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
                                auth: Auth::Wpa,
                                psk: None,
                                hidden: false,
                            }),
                            wifi_enabled: Some(false),
                            smart_switching: None,
                            airplane_mode: Some(false),
                            created_at: DateTime::from_timestamp(1758277671, 0).unwrap(),
                        }),
            (
            "NETCONFIG:v1.0;WIFI:T:SAE;S:network;P:password;;TS:1758277671;", NetConfig {
                            wifi_credentials: Some(wifi::Credentials {
                                ssid: "network".to_string(),
                                auth: Auth::Sae,
                                psk: Some("password".into()),
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
            assert_eq!(actual, expected, "INPUT: {netconfig_str}");
        }
    }
}
