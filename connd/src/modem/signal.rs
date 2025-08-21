use color_eyre::Result;
use serde::{Deserialize, Deserializer, Serialize};

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

/// LTE Signal measurements
#[derive(Debug, Deserialize, Serialize)]
pub struct LteSignal {
    /// Reference Signal Received Power — how strong the LTE signal is.
    #[serde(deserialize_with = "de_string_to_f64_opt")]
    pub rsrp: Option<f64>,

    ///Reference Signal Received Quality — signal quality, affected by interference.
    #[serde(deserialize_with = "de_string_to_f64_opt")]
    pub rsrq: Option<f64>,

    /// Received Signal Strength Indicator — total signal power (including noise)
    #[serde(deserialize_with = "de_string_to_f64_opt")]
    pub rssi: Option<f64>,

    /// Signal-to-Noise Ratio) — how "clean" the signal is.
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
}
