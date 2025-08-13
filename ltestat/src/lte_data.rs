use super::utils::run_cmd;
use color_eyre::{eyre::eyre, Result};
use serde::{Deserialize, Deserializer, Serialize};

#[derive(Serialize)]
pub struct LteStat {
    pub signal: Option<LteSignal>,

    pub location: Option<GppLocation>,

    pub net_stats: Option<NetStats>,
}

impl LteStat {
    pub async fn poll_for(modem_id: &str) -> Result<Self> {
        let signal_output =
            run_cmd("mmcli", &["-m", modem_id, "--signal-get", "--output-json"])
                .await?;
        let signal: MmcliSignalRoot = serde_json::from_str(&signal_output)?;
        let signal = signal.modem.signal.lte;

        let location_output = run_cmd(
            "mmcli",
            &["-m", modem_id, "--location-get", "--output-json"],
        )
        .await?;

        let location: MmcliLocationRoot = serde_json::from_str(&location_output)?;

        let location = location.modem.location.gpp;

        let net_stats = NetStats::new().await?;

        Ok(Self {
            // timestamp,
            signal,
            location,
            net_stats: Some(net_stats),
        })
    }
}

/// Output of mmcli -m 0 --signal-get
#[derive(Debug, Deserialize)]
pub struct MmcliSignalRoot {
    pub modem: MmcliSignalModem,
}

#[derive(Debug, Deserialize)]
pub struct MmcliSignalModem {
    pub signal: MmcliSignalData,
}

#[derive(Debug, Deserialize)]
pub struct MmcliSignalData {
    pub lte: Option<LteSignal>,
    pub _refresh: Option<RefreshRate>,
}

#[derive(Debug, Deserialize, Serialize)]

/// LTE Signal measurements
pub struct LteSignal {
    #[serde(deserialize_with = "de_string_to_f64_opt")]

    /// Reference Signal Received Power — how strong the LTE signal is.
    pub rsrp: Option<f64>,
    #[serde(deserialize_with = "de_string_to_f64_opt")]

    ///Reference Signal Received Quality — signal quality, affected by interference.
    pub rsrq: Option<f64>,
    #[serde(deserialize_with = "de_string_to_f64_opt")]

    /// Received Signal Strength Indicator — total signal power (including noise)
    pub rssi: Option<f64>,
    #[serde(deserialize_with = "de_string_to_f64_opt", rename = "snr")]

    /// Signal-to-Noise Ratio) — how "clean" the signal is.
    pub snr: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub struct RefreshRate {
    #[serde(deserialize_with = "de_string_to_u32_opt")]
    pub _rate: Option<u32>,
}

/// Parse the signal info json to f64. If the field is not presenet
/// mmcli marks it as "--" => None
pub fn de_string_to_f64_opt<'de, D>(desrializer: D) -> Result<Option<f64>, D::Error>
where
    D: Deserializer<'de>,
{
    let s: Option<&str> = Option::deserialize(desrializer)?;

    match s {
        Some("--") | None => Ok(None),
        Some(val) => val
            .parse::<f64>()
            .map(Some)
            .map_err(serde::de::Error::custom),
    }
}

/// Parse the signal info json to f64. If the field is not presenet
/// mmcli marks it as "--" => None
pub fn de_string_to_u32_opt<'de, D>(deserializer: D) -> Result<Option<u32>, D::Error>
where
    D: Deserializer<'de>,
{
    let s: Option<&str> = Option::deserialize(deserializer)?;
    match s {
        Some("--") | None => Ok(None),
        Some(val) => val
            .parse::<u32>()
            .map(Some)
            .map_err(serde::de::Error::custom),
    }
}

#[derive(Debug, Deserialize)]
pub struct MmcliLocationRoot {
    pub modem: MmcliLocationModem,
}

#[derive(Debug, Deserialize)]
pub struct MmcliLocationModem {
    pub location: MmcliLocationData,
}

#[derive(Debug, Deserialize)]
pub struct MmcliLocationData {
    #[serde(rename = "3gpp")]
    pub gpp: Option<GppLocation>,
}

#[derive(Debug, Deserialize, Serialize)]

/// Information about the currently connected LTE cell tower.
pub struct GppLocation {
    /// Cell ID — unique identifier for the specific cell tower sector.
    pub cid: Option<String>,

    /// Location Area Code — identifies a group of nearby towers.
    pub lac: Option<String>,

    /// Mobile Country Code — identifies the country.
    pub mcc: Option<String>,

    /// Mobile Network Code — identifies the carrier.
    pub mnc: Option<String>,

    /// Tracking Area Code — like LAC, but specific to LTE.
    pub tac: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct NetStats {
    pub tx_bytes: u64,
    pub rx_bytes: u64,
}

impl NetStats {
    pub async fn new() -> Result<Self> {
        let output = run_cmd(
            "cat",
            &[
                "/sys/class/net/wwan0/statistics/tx_bytes",
                "/sys/class/net/wwan0/statistics/rx_bytes",
            ],
        )
        .await?;

        let mut lines = output.lines();

        let tx_bytes: u64 = lines
            .next()
            .ok_or_else(|| eyre!("Missing tx_bytes info line."))?
            .trim()
            .parse()?;

        let rx_bytes: u64 = lines
            .next()
            .ok_or_else(|| eyre!("Missing rx_bytes info line"))?
            .trim()
            .parse()?;

        Ok(Self { tx_bytes, rx_bytes })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_deserializes_mmcli_signal_output() {
        let json_input = r#"
        {
            "modem": {
                "signal": {
                    "5g": {
                        "error-rate": "--",
                        "rsrp": "--",
                        "rsrq": "--",
                        "snr": "--"
                    },
                    "cdma1x": {
                        "ecio": "--",
                        "error-rate": "--",
                        "rssi": "--"
                    },
                    "evdo": {
                        "ecio": "--",
                        "error-rate": "--",
                        "io": "--",
                        "rssi": "--",
                        "sinr": "--"
                    },
                    "gsm": {
                        "error-rate": "--",
                        "rssi": "--"
                    },
                    "lte": {
                        "error-rate": "--",
                        "rsrp": "-112.00",
                        "rsrq": "-17.00",
                        "rssi": "-74.00",
                        "snr": "-2.00"
                    },
                    "refresh": {
                        "rate": "10"
                    },
                    "threshold": {
                        "error-rate": "no",
                        "rssi": "0"
                    },
                    "umts": {
                        "ecio": "--",
                        "error-rate": "--",
                        "rscp": "--",
                        "rssi": "--"
                    }
                }
            }
        }
        "#;

        let parsed: MmcliSignalRoot = serde_json::from_str(json_input).unwrap();
        let lte = parsed.modem.signal.lte.unwrap();

        assert_eq!(lte.rsrp, Some(-112.0));
        assert_eq!(lte.rsrq, Some(-17.0));
        assert_eq!(lte.rssi, Some(-74.0));
        assert_eq!(lte.snr, Some(-2.0));
    }

    #[test]
    fn is_deserializes_mmcli_location_output() {
        let json = r#"
        {
            "modem": {
                "location": {
                    "3gpp": {
                        "cid": "0197763E",
                        "lac": "0000",
                        "mcc": "262",
                        "mnc": "03",
                        "tac": "00C945"
                    },
                    "cdma-bs": {
                        "latitude": "--",
                        "longitude": "--"
                    },
                    "gps": {
                        "altitude": "--",
                        "latitude": "--",
                        "longitude": "--",
                        "nmea": [],
                        "utc": "--"
                    }
                }
            }
        }
        "#;

        let parsed: MmcliLocationRoot =
            serde_json::from_str(json).expect("Failed to parse");

        let gpp = parsed.modem.location.gpp.expect("Missing 3GPP data");

        assert_eq!(gpp.cid.as_deref(), Some("0197763E"));
        assert_eq!(gpp.lac.as_deref(), Some("0000"));
        assert_eq!(gpp.mcc.as_deref(), Some("262"));
        assert_eq!(gpp.mnc.as_deref(), Some("03"));
        assert_eq!(gpp.tac.as_deref(), Some("00C945"));
    }
}
