use serde::{Serialize, Deserialize};

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

/// Information about the currently connected LTE cell tower.
#[derive(Debug, Deserialize, Serialize)]
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

#[cfg(test)]
mod tests {
    use super::*;

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
