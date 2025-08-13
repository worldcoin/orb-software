use super::utils::run_cmd;
use color_eyre::{eyre::eyre, Result};
use serde::{Deserialize, Deserializer, Serialize};

#[derive(Serialize)]
pub struct LteStat {
    // TODO: Timezone aware ???
    // pub timestamp: DateTime<Utc>,
    pub signal: Option<LteSignal>,

    pub location: Option<GppLocation>,

    pub net_stats: Option<NetStats>,

    pub battery_status: Option<BatteryStatus>,
}

impl LteStat {
    pub async fn poll() -> Result<Self> {
        // TODO: Put some thought into this bro please
        // let timestamp = Utc::now();

        let signal_output =
            run_cmd("mmcli", &["-m", "0", "--signal-get", "--output-json"]).await?;
        let signal: MmcliSignalRoot = serde_json::from_str(&signal_output)?;
        let signal = signal.modem.signal.lte;

        let location_output = run_cmd(
            "sudo",
            &["mmcli", "-m", "0", "--location-get", "--output-json"],
        )
        .await?;

        let location: MmcliLocationRoot = serde_json::from_str(&location_output)?;

        let location = location.modem.location.gpp;

        let net_stats = NetStats::new().await?;
        let battery_status = BatteryStatus::new().await?;

        Ok(Self {
            // timestamp,
            signal,
            location,
            net_stats: Some(net_stats),
            battery_status: Some(battery_status),
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
pub struct LteSignal {
    #[serde(deserialize_with = "de_string_to_f64_opt")]
    pub rsrp: Option<f64>,
    #[serde(deserialize_with = "de_string_to_f64_opt")]
    pub rsrq: Option<f64>,
    #[serde(deserialize_with = "de_string_to_f64_opt")]
    pub rssi: Option<f64>,
    #[serde(deserialize_with = "de_string_to_f64_opt", rename = "snr")]
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
pub struct GppLocation {
    pub cid: Option<String>,
    pub lac: Option<String>,
    pub mcc: Option<String>,
    pub mnc: Option<String>,
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

#[derive(Serialize)]
pub struct BatteryStatus {
    percentage: u8,
}

impl BatteryStatus {
    fn from_string(_cmd_output: String) -> Result<Self> {
        // let percentage = cmd_output
        //     .lines()
        //     .find(|line| line.contains("battery:"))
        //     .ok_or_else(|| eyre!("Battery charge not found."))?
        //     .trim()
        //     .split("battery: ")
        //     .nth(1)
        //     .ok_or_else(|| eyre!("Malformed battery charge line."))?
        //     .trim()
        //     .trim_end_matches("%")
        //     .parse::<u8>()?;
        //
        let percentage = 100;
        Ok(Self { percentage })
    }

    pub async fn new() -> Result<Self> {
        let output = run_cmd("orb-mcu-util", &["info"]).await?;
        BatteryStatus::from_string(output)
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

        let refresh = parsed.modem.signal.refresh.unwrap();

        assert_eq!(refresh.rate, Some(10));
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

    #[test]
    fn it_parses_battery_charge() {
        let cmd_output = r#"
            🔮 Orb info:
                revision:       Diamond_EVT
                battery:        unknown
            🚜 Main board:
                current image:  v3.0.17-0x643ed8da (prod)
                secondary slot: v3.1.7-0x6a7e5995 (prod)
            🔐 Security board:
                current image:  v3.0.8-0x75276004 (prod)
                secondary slot: v3.1.7-0x5a5c5903 (prod)
                battery charge: 100%
                voltage:        4130mV
                charging:       no
        "#;

        let battery_status = BatteryStatus::from_string(cmd_output.into()).unwrap();

        assert_eq!(battery_status.percentage, 100);

        let cmd_output = r#"
            🔮 Orb info:
                revision:       Diamond_EVT
                battery:        unknown
            🚜 Main board:
                current image:  v3.0.17-0x643ed8da (prod)
                secondary slot: v3.1.7-0x6a7e5995 (prod)
            🔐 Security board:
                current image:  v3.0.8-0x75276004 (prod)
                secondary slot: v3.1.7-0x5a5c5903 (prod)
                battery charge: 88%
                voltage:        4130mV
                charging:       no
        "#;

        let battery_status = BatteryStatus::from_string(cmd_output.into()).unwrap();

        assert_eq!(battery_status.percentage, 88)
    }
}
