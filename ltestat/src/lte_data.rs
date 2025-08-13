use super::connection_state::ConnectionState;
use super::utils::run_cmd;
use chrono::{DateTime, Utc};
use color_eyre::{
    eyre::{eyre, OptionExt},
    Result,
};
use serde::{Deserialize, Deserializer, Serialize};
use tokio::time::Instant;

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

/// Holds modem identity and connection tracking — no verbose logs.
pub struct ModemMonitor {
    pub modem_id: String,

    pub state: ConnectionState,
    pub last_state: Option<ConnectionState>,

    /// Monotonic start of current disconnect (if any)
    pub disconnected_since: Option<Instant>,

    /// Wall-clock timestamps for last transitions (for reporting)
    pub last_disconnected_at: Option<DateTime<Utc>>,
    pub last_connected_at: Option<DateTime<Utc>>,

    /// How many times we’ve transitioned from connected -> not connected
    pub disconnected_count: u64,

    /// Optional: last telemetry snapshot; useful for consumers
    pub last_snapshot: Option<LteStat>,

    /// Optional: duration of the most recent downtime (secs)
    pub last_downtime_secs: Option<f64>,
}

impl ModemMonitor {
    pub async fn new() -> Result<Self> {
        // Get the modem ID used by mmcli
        let output = run_cmd("mmcli", &["-L"]).await?;
        let modem_id = output
            .split_whitespace()
            .next()
            .and_then(|path| path.rsplit('/').next())
            .ok_or_eyre("Failed to get modem id")?
            .to_owned();

        Ok(Self {
            modem_id,
            state: ConnectionState::Unknown,
            last_state: None,
            disconnected_since: None,
            last_disconnected_at: None,
            last_connected_at: None,
            disconnected_count: 0,
            last_snapshot: None,
            last_downtime_secs: None,
        })
    }

    pub async fn wait_for_connection(&mut self) -> Result<()> {
        println!("Waiting for modem {} to reconnect...", self.modem_id);
    }

    /// Update internal state. No printing/logging — we only store times/counters.
    pub fn update_state(
        &mut self,
        now_inst: Instant,
        now_utc: DateTime<Utc>,
        current: ConnectionState,
    ) {
        let was_connected = self.last_state.as_ref().map_or(false, |s| s.is_online());
        let is_connected = current.is_online();

        if was_connected && !is_connected {
            // connected -> not connected
            self.disconnected_since = Some(now_inst);
            self.last_disconnected_at = Some(now_utc);
            self.disconnected_count += 1;
            self.last_downtime_secs = None; // reset; will set on reconnection
        } else if !was_connected && is_connected {
            // not connected -> connected
            if let Some(start) = self.disconnected_since.take() {
                self.last_downtime_secs =
                    Some(now_inst.duration_since(start).as_secs_f64());
            }
            self.last_connected_at = Some(now_utc);
        }

        self.last_state = Some(current);
        self.state = current;
    }

    pub async fn poll_lte(&mut self) -> Result<&LteStat> {
        let snap = LteStat::poll_for(&self.modem_id).await?;
        self.last_snapshot = Some(snap);
        Ok(self.last_snapshot.as_ref().unwrap())
    }

    pub fn dump_info(&self) {
        println!("=== Modem Monitor Status ===");
        println!("Modem ID: {}", self.modem_id);
        println!("Current State: {:?}", self.state);
        println!("Last State: {:?}", self.last_state);

        println!("Disconnected Count: {}", self.disconnected_count);

        if let Some(dt) = &self.last_disconnected_at {
            println!("Last Disconnected At: {}", dt.to_rfc3339());
        } else {
            println!("Last Disconnected At: never");
        }

        if let Some(dt) = &self.last_connected_at {
            println!("Last Connected At: {}", dt.to_rfc3339());
        } else {
            println!("Last Connected At: never");
        }

        if let Some(secs) = self.last_downtime_secs {
            println!("Last Downtime: {:.1} seconds", secs);
        } else if self.disconnected_since.is_some() {
            println!("Currently Disconnected — downtime still in progress");
        } else {
            println!("Last Downtime: n/a");
        }

        println!("============================");
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
