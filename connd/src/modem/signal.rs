use color_eyre::Result;
use serde::{Deserialize, Deserializer, Serialize};

/// Top-level deserialization root for `mmcli -m <id> --signal-get --output-json`.
/// Contains the modem section, which holds signal metrics grouped by RAT.
#[derive(Debug, Deserialize)]
pub struct MmcliSignalRoot {
    /// The modem object reported by `mmcli`.
    pub modem: MmcliSignalModem,
}

/// Wrapper for the modem section inside the mmcli JSON.
/// Holds the nested `signal` block with per-RAT measurements.
#[derive(Debug, Deserialize)]
pub struct MmcliSignalModem {
    /// The signal measurements subtree.
    pub signal: MmcliSignalData,
}

/// All possible signal measurement blocks that mmcli may return.
/// Each RAT (Radio Access Technology) appears only if the modem supports it
/// and if ModemManager reports it. Missing sections are `None`.
#[derive(Debug, Deserialize)]
pub struct MmcliSignalData {
    /// 5G NR (New Radio) signal metrics.
    /// JSON key is `"5g"`.
    #[serde(rename = "5g")]
    pub nr5g: Option<Nr5gSignal>,

    /// LTE (4G) signal metrics.
    pub lte: Option<LteSignal>,

    /// UMTS / HSPA (3G) signal metrics.
    pub umts: Option<UmtsSignal>,

    /// GSM / GPRS / EDGE (2G) signal metrics.
    pub gsm: Option<GsmSignal>,

    /// CDMA 1x signal metrics.
    pub cdma1x: Option<Cdma1xSignal>,

    /// EVDO signal metrics.
    pub evdo: Option<EvdoSignal>,

    /// Signal refresh configuration (if set via `--signal-setup`).
    /// JSON key is `"refresh"`.
    #[serde(rename = "refresh")]
    pub _refresh: Option<RefreshRate>,
}
impl MmcliSignalData {
    /// Return a normalized view of signal metrics for the given RAT string from mmcli.
    /// Unknown or unsupported RATs yield all-None metrics.
    fn metrics_for_rat(&self, rat: &str) -> SignalMetrics {
        match AccessTech::from_rat_value(rat) {
            Some(AccessTech::Gsm) => {
                if let Some(g) = &self.gsm {
                    SignalMetrics {
                        rssi: g.rssi,
                        error_rate: g.error_rate,
                        ..Default::default()
                    }
                } else {
                    SignalMetrics::default()
                }
            }
            Some(AccessTech::Umts) => {
                if let Some(u) = &self.umts {
                    SignalMetrics {
                        rssi: u.rssi,
                        ecio: u.ecio,
                        rscp: u.rscp,
                        error_rate: u.error_rate,
                        ..Default::default()
                    }
                } else {
                    SignalMetrics::default()
                }
            }
            Some(AccessTech::Lte) => {
                if let Some(l) = &self.lte {
                    SignalMetrics {
                        rsrp: l.rsrp,
                        rsrq: l.rsrq,
                        rssi: l.rssi,
                        snr: l.snr,
                        error_rate: l.error_rate,
                        ..Default::default()
                    }
                } else {
                    SignalMetrics::default()
                }
            }
            Some(AccessTech::Nr5g) => {
                if let Some(n) = &self.nr5g {
                    SignalMetrics {
                        rsrp: n.rsrp,
                        rsrq: n.rsrq,
                        snr: n.snr,
                        error_rate: n.error_rate,
                        ..Default::default()
                    }
                } else {
                    SignalMetrics::default()
                }
            }
            Some(AccessTech::Cdma1x) => {
                if let Some(c) = &self.cdma1x {
                    SignalMetrics {
                        rssi: c.rssi,
                        ecio: c.ecio,
                        error_rate: c.error_rate,
                        ..Default::default()
                    }
                } else {
                    SignalMetrics::default()
                }
            }
            Some(AccessTech::Evdo) => {
                if let Some(e) = &self.evdo {
                    SignalMetrics {
                        rssi: e.rssi,
                        ecio: e.ecio,
                        sinr: e.sinr,
                        io: e.io,
                        error_rate: e.error_rate,
                        ..Default::default()
                    }
                } else {
                    SignalMetrics::default()
                }
            }
            None => SignalMetrics::default(),
        }
    }

    pub fn get_metrics_with_fallback(&self, rat: &Option<String>) -> SignalMetrics {
        if let Some(r) = rat {
            let m = self.metrics_for_rat(r.as_str());
            if m != SignalMetrics::default() {
                return m;
            }
        }
        // fallback: pick the first populated RAT block
        // in case we have no RAT
        if self.lte.is_some() {
            return self.metrics_for_rat("lte");
        }
        if self.nr5g.is_some() {
            return self.metrics_for_rat("5g");
        }
        if self.umts.is_some() {
            return self.metrics_for_rat("umts");
        }
        if self.gsm.is_some() {
            return self.metrics_for_rat("gsm");
        }
        if self.cdma1x.is_some() {
            return self.metrics_for_rat("cdma1x");
        }
        if self.evdo.is_some() {
            return self.metrics_for_rat("evdo");
        }
        SignalMetrics::default()
    }
}

/// LTE Signal measurements
#[derive(Debug, Deserialize, Serialize)]
pub struct LteSignal {
    /// Reference Signal Received Power
    #[serde(default, deserialize_with = "de_string_to_f64_opt")]
    pub rsrp: Option<f64>,

    /// Reference Signal Received Quality
    #[serde(default, deserialize_with = "de_string_to_f64_opt")]
    pub rsrq: Option<f64>,

    /// Received Signal Strength Indicator
    #[serde(default, deserialize_with = "de_string_to_f64_opt")]
    pub rssi: Option<f64>,

    /// Signal-to-Noise Ratio
    #[serde(default, deserialize_with = "de_string_to_f64_opt", rename = "snr")]
    pub snr: Option<f64>,

    /// Error rate, if reported by modem
    #[serde(
        default,
        deserialize_with = "de_string_to_f64_opt",
        rename = "error-rate"
    )]
    pub error_rate: Option<f64>,
}

/// NR (5G) Signal measurements

#[derive(Debug, Deserialize, Serialize)]
pub struct Nr5gSignal {
    /// Reference Signal Received Power
    #[serde(default, deserialize_with = "de_string_to_f64_opt")]
    pub rsrp: Option<f64>,

    /// Reference Signal Received Quality
    #[serde(default, deserialize_with = "de_string_to_f64_opt")]
    pub rsrq: Option<f64>,

    /// Signal-to-Noise Ratio
    #[serde(default, deserialize_with = "de_string_to_f64_opt", rename = "snr")]
    pub snr: Option<f64>,

    /// Error rate, if reported by modem
    #[serde(
        default,
        deserialize_with = "de_string_to_f64_opt",
        rename = "error-rate"
    )]
    pub error_rate: Option<f64>,
}

/// UMTS/3G Signal measurements
#[derive(Debug, Deserialize, Serialize)]
pub struct UmtsSignal {
    /// Received Signal Strength Indicator
    #[serde(default, deserialize_with = "de_string_to_f64_opt")]
    pub rssi: Option<f64>,

    /// Energy per chip over interference
    #[serde(default, deserialize_with = "de_string_to_f64_opt")]
    pub ecio: Option<f64>,

    /// Received Signal Code Power
    #[serde(default, deserialize_with = "de_string_to_f64_opt")]
    pub rscp: Option<f64>,

    /// Error rate, if reported by modem
    #[serde(
        default,
        deserialize_with = "de_string_to_f64_opt",
        rename = "error-rate"
    )]
    pub error_rate: Option<f64>,
}

/// GSM/2G Signal measurements
#[derive(Debug, Deserialize, Serialize)]
pub struct GsmSignal {
    /// Received Signal Strength Indicator
    #[serde(default, deserialize_with = "de_string_to_f64_opt")]
    pub rssi: Option<f64>,

    /// Error rate, if reported by modem
    #[serde(
        default,
        deserialize_with = "de_string_to_f64_opt",
        rename = "error-rate"
    )]
    pub error_rate: Option<f64>,
}

/// CDMA1x Signal measurements
#[derive(Debug, Deserialize, Serialize)]
pub struct Cdma1xSignal {
    /// Received Signal Strength Indicator
    #[serde(default, deserialize_with = "de_string_to_f64_opt")]
    pub rssi: Option<f64>,

    /// Energy per chip over interference
    #[serde(default, deserialize_with = "de_string_to_f64_opt")]
    pub ecio: Option<f64>,

    /// Error rate, if reported by modem
    #[serde(
        default,
        deserialize_with = "de_string_to_f64_opt",
        rename = "error-rate"
    )]
    pub error_rate: Option<f64>,
}

/// EVDO Signal measurements
#[derive(Debug, Deserialize, Serialize)]
pub struct EvdoSignal {
    /// Received Signal Strength Indicator
    #[serde(default, deserialize_with = "de_string_to_f64_opt")]
    pub rssi: Option<f64>,

    /// Energy per chip over interference
    #[serde(default, deserialize_with = "de_string_to_f64_opt")]
    pub ecio: Option<f64>,

    /// Signal-to-Interference-plus-Noise Ratio
    #[serde(default, deserialize_with = "de_string_to_f64_opt")]
    pub sinr: Option<f64>,

    /// Interference over thermal noise
    #[serde(default, deserialize_with = "de_string_to_f64_opt", rename = "io")]
    pub io: Option<f64>,

    /// Error rate, if reported by modem
    #[serde(
        default,
        deserialize_with = "de_string_to_f64_opt",
        rename = "error-rate"
    )]
    pub error_rate: Option<f64>,
}
#[derive(Debug, Deserialize)]
pub struct RefreshRate {
    #[serde(deserialize_with = "de_string_to_u32_opt", rename = "rate")]
    pub _rate: Option<u32>,
}

/// Parse the signal info JSON to f64. If the field is not present,
/// mmcli marks it as "--" => None
pub fn de_string_to_f64_opt<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: Deserializer<'de>,
{
    let s: Option<&str> = Option::deserialize(deserializer)?;
    match s {
        Some("--") | None => Ok(None),
        Some(val) => val
            .parse::<f64>()
            .map(Some)
            .map_err(serde::de::Error::custom),
    }
}

/// Parse the signal info JSON to u32. If the field is not present,
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

/// A normalized view over all RATs. Non-applicable metrics are None.
#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct SignalMetrics {
    /// Reference Signal Received Power (LTE/NR).
    /// Typical range: -140 dBm (weak) to -44 dBm (strong).
    pub rsrp: Option<f64>,

    /// Reference Signal Received Quality (LTE/NR).
    /// Typical range: -20 dB (poor) to -3 dB (excellent).
    pub rsrq: Option<f64>,

    /// Received Signal Strength Indicator (GSM/UMTS/LTE/CDMA/EVDO).
    /// Rough indication of total received power, including noise.
    pub rssi: Option<f64>,

    /// Signal-to-Noise Ratio (LTE/NR).
    /// Higher values mean a cleaner signal, usually -20 dB to +30 dB.
    pub snr: Option<f64>,

    /// Signal-to-Interference-plus-Noise Ratio (EVDO).
    pub sinr: Option<f64>,

    /// Energy per chip over interference (UMTS/CDMA/EVDO).
    pub ecio: Option<f64>,

    /// Received Signal Code Power (UMTS).
    pub rscp: Option<f64>,

    /// Interference over thermal noise (EVDO).
    pub io: Option<f64>,

    /// Generic error rate
    pub error_rate: Option<f64>,
}

#[derive(Debug, Clone, Copy)]
enum AccessTech {
    Gsm,
    Umts,
    Lte,
    Nr5g,
    Cdma1x,
    Evdo,
}

impl AccessTech {
    /// Normalize various mmcli RAT values
    fn from_rat_value(s: &str) -> Option<Self> {
        let s = s.to_ascii_lowercase();
        match s.as_str() {
            "gsm" | "gprs" | "edge" => Some(Self::Gsm),
            "umts" | "hspa" | "hspa+" => Some(Self::Umts),
            "lte" => Some(Self::Lte),
            "5g" | "5g-nsa" | "nr5g" | "nr5g-nsa" | "nr5g-sa" => Some(Self::Nr5g),
            "cdma1x" | "cdma-1x" => Some(Self::Cdma1x),
            "evdo" | "cdma-evdo" => Some(Self::Evdo),
            // If mmcli returns a semicolon-separated list, take the first token.
            _ if s.contains(';') => {
                let first = s.split(';').next().unwrap().trim();
                Self::from_rat_value(first)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_deserializes_mmcli_signal_output_lte() {
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
                    "refresh": { "rate": "10" },
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
        }"#;

        let parsed: MmcliSignalRoot = serde_json::from_str(json_input).unwrap();
        let lte = parsed.modem.signal.lte.as_ref().unwrap();

        assert_eq!(lte.rsrp, Some(-112.0));
        assert_eq!(lte.rsrq, Some(-17.0));
        assert_eq!(lte.rssi, Some(-74.0));
        assert_eq!(lte.snr, Some(-2.0));

        let norm = parsed.modem.signal.metrics_for_rat("LTE");
        assert_eq!(norm.rsrp, Some(-112.0));
        assert_eq!(norm.rsrq, Some(-17.0));
        assert_eq!(norm.rssi, Some(-74.0));
        assert_eq!(norm.snr, Some(-2.0));
        assert_eq!(norm.ecio, None);
    }

    #[test]
    fn it_maps_various_rat_strings() {
        assert!(matches!(
            super::AccessTech::from_rat_value("lte"),
            Some(super::AccessTech::Lte)
        ));
        assert!(matches!(
            super::AccessTech::from_rat_value("5g-nsa"),
            Some(super::AccessTech::Nr5g)
        ));
        assert!(matches!(
            super::AccessTech::from_rat_value("HSPA+"),
            Some(super::AccessTech::Umts)
        ));
        assert!(matches!(
            super::AccessTech::from_rat_value("edge"),
            Some(super::AccessTech::Gsm)
        ));
        assert!(matches!(
            super::AccessTech::from_rat_value("cdma-1x"),
            Some(super::AccessTech::Cdma1x)
        ));
        assert!(matches!(
            super::AccessTech::from_rat_value("evdo"),
            Some(super::AccessTech::Evdo)
        ));
        assert!(matches!(
            super::AccessTech::from_rat_value("lte;nr5g-nsa"),
            Some(super::AccessTech::Lte)
        ));
    }

    #[test]
    fn it_deserializes_nr5g_and_normalizes() {
        let json = r#"
        {
          "modem": {
            "signal": {
              "5g": { "rsrp": "-95.0", "rsrq": "-10.5", "snr": "15.2", "error-rate": "--" }
            }
          }
        }"#;

        let parsed: MmcliSignalRoot = serde_json::from_str(json).unwrap();
        let norm = parsed.modem.signal.metrics_for_rat("5g");

        assert_eq!(norm.rsrp, Some(-95.0));
        assert_eq!(norm.rsrq, Some(-10.5));
        assert_eq!(norm.snr, Some(15.2));
        assert_eq!(norm.error_rate, None);
        assert_eq!(norm.rssi, None);
        assert_eq!(norm.ecio, None);
    }

    #[test]
    fn it_deserializes_umts_and_normalizes() {
        let json = r#"
        {
          "modem": {
            "signal": {
              "umts": { "rssi": "-70.0", "ecio": "-6.0", "rscp": "-95.0", "error-rate": "0.01" }
            }
          }
        }"#;

        let parsed: MmcliSignalRoot = serde_json::from_str(json).unwrap();
        let norm = parsed.modem.signal.metrics_for_rat("umts");

        assert_eq!(norm.rssi, Some(-70.0));
        assert_eq!(norm.ecio, Some(-6.0));
        assert_eq!(norm.rscp, Some(-95.0));
        assert_eq!(norm.error_rate, Some(0.01));
        assert_eq!(norm.rsrp, None);
        assert_eq!(norm.snr, None);
    }

    #[test]
    fn it_deserializes_gsm_and_normalizes() {
        let json = r#"
        {
          "modem": {
            "signal": {
              "gsm": { "rssi": "-85.0", "error-rate": "--" }
            }
          }
        }"#;

        let parsed: MmcliSignalRoot = serde_json::from_str(json).unwrap();
        let norm = parsed.modem.signal.metrics_for_rat("gsm");

        assert_eq!(norm.rssi, Some(-85.0));
        assert_eq!(norm.error_rate, None);
        // Non-applicable for GSM:
        assert_eq!(norm.rsrp, None);
        assert_eq!(norm.rsrq, None);
        assert_eq!(norm.snr, None);
    }

    #[test]
    fn it_deserializes_cdma1x_and_normalizes() {
        let json = r#"
        {
          "modem": {
            "signal": {
              "cdma1x": { "rssi": "-82.0", "ecio": "-8.5", "error-rate": "0.02" }
            }
          }
        }"#;

        let parsed: MmcliSignalRoot = serde_json::from_str(json).unwrap();
        let norm = parsed.modem.signal.metrics_for_rat("cdma1x");

        assert_eq!(norm.rssi, Some(-82.0));
        assert_eq!(norm.ecio, Some(-8.5));
        assert_eq!(norm.error_rate, Some(0.02));
        assert_eq!(norm.rsrp, None);
        assert_eq!(norm.snr, None);
    }

    #[test]
    fn it_deserializes_evdo_and_normalizes() {
        let json = r#"
        {
          "modem": {
            "signal": {
              "evdo": { "rssi": "-80.0", "ecio": "-7.0", "sinr": "9.0", "io": "-100.0", "error-rate": "--" }
            }
          }
        }"#;

        let parsed: MmcliSignalRoot = serde_json::from_str(json).unwrap();
        let norm = parsed.modem.signal.metrics_for_rat("evdo");

        assert_eq!(norm.rssi, Some(-80.0));
        assert_eq!(norm.ecio, Some(-7.0));
        assert_eq!(norm.sinr, Some(9.0));
        assert_eq!(norm.io, Some(-100.0));
        assert_eq!(norm.error_rate, None);
        assert_eq!(norm.rsrp, None);
        assert_eq!(norm.rsrq, None);
        assert_eq!(norm.snr, None);
    }

    #[test]
    fn it_handles_dash_dash_as_none_for_all_fields() {
        let json = r#"
        {
          "modem": {
            "signal": {
              "lte": { "rsrp": "--", "rsrq": "--", "rssi": "--", "snr": "--", "error-rate": "--" }
            }
          }
        }"#;

        let parsed: MmcliSignalRoot = serde_json::from_str(json).unwrap();
        let norm = parsed.modem.signal.metrics_for_rat("lte");

        assert_eq!(norm.rsrp, None);
        assert_eq!(norm.rsrq, None);
        assert_eq!(norm.rssi, None);
        assert_eq!(norm.snr, None);
        assert_eq!(norm.error_rate, None);
    }

    #[test]
    fn it_maps_semicolon_separated_rat_taking_first_token() {
        // AccessTech::from_rat_value("lte;nr5g-nsa") => Lte
        assert!(matches!(
            super::AccessTech::from_rat_value("lte;nr5g-nsa"),
            Some(super::AccessTech::Lte)
        ));

        // And the normalizer is case-insensitive:
        assert!(matches!(
            super::AccessTech::from_rat_value("HSPA+;LTE"),
            Some(super::AccessTech::Umts)
        ));
    }

    #[test]
    fn it_returns_default_metrics_for_unknown_rat() {
        let json = r#"
        {
          "modem": {
            "signal": {
              "lte": { "rsrp": "-100.0", "rsrq": "-9.5", "rssi": "-70.0", "snr": "5.5" }
            }
          }
        }"#;

        let parsed: MmcliSignalRoot = serde_json::from_str(json).unwrap();

        // Unknown RAT string → default metrics (all None)
        let norm = parsed
            .modem
            .signal
            .metrics_for_rat("satellite-proprietary-x");
        assert_eq!(norm, SignalMetrics::default());
    }

    #[test]
    fn it_parses_refresh_rate_when_present() {
        let json = r#"
        {
          "modem": {
            "signal": {
              "lte": { "rsrp": "-100.0", "rsrq": "-9.5", "rssi": "-70.0", "snr": "5.5" },
              "refresh": { "rate": "10" }
            }
          }
        }"#;

        let parsed: MmcliSignalRoot = serde_json::from_str(json).unwrap();
        let refresh = parsed.modem.signal._refresh.as_ref().unwrap();
        assert_eq!(refresh._rate, Some(10));
    }

    #[test]
    fn it_tolerates_missing_sections() {
        // Only GSM present; asking for LTE yields default/empty metrics.
        let json = r#"
        {
          "modem": {
            "signal": {
              "gsm": { "rssi": "-90.0", "error-rate": "0.05" }
            }
          }
        }"#;

        let parsed: MmcliSignalRoot = serde_json::from_str(json).unwrap();
        let gsm = parsed.modem.signal.metrics_for_rat("gsm");
        assert_eq!(gsm.rssi, Some(-90.0));
        assert_eq!(gsm.error_rate, Some(0.05));

        let lte = parsed.modem.signal.metrics_for_rat("lte");
        assert_eq!(lte, SignalMetrics::default());
    }
}
