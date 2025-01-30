use std::{sync::OnceLock, time::Duration};

use eyre::Result;
use orb_endpoints::{v2::Endpoints as EndpointsV2, Backend, OrbId};
use orb_security_utils::reqwest::reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::data::{CellularInfo, WifiNetwork};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OrbStatusV2 {
    pub orb_id: Option<String>,
    pub location_data: Option<LocationDataV2>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LocationDataV2 {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wifi: Option<Vec<WifiDataV2>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cell: Option<Vec<CellDataV2>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WifiDataV2 {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bssid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signal_strength: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signal_to_noise_ratio: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CellDataV2 {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcc: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mnc: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lac: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cell_id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signal_strength: Option<i32>,
}

pub fn client() -> &'static Client {
    static CLIENT: OnceLock<Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        let builder = orb_security_utils::reqwest::blocking::http_client_builder()
            .timeout(Duration::from_secs(60))
            .user_agent("orb-cellcom");
        builder.build().expect("Failed to build client")
    })
}

pub fn get_location(
    orb_id: &OrbId,
    cellular_info: &CellularInfo,
    wifi_networks: &[WifiNetwork],
) -> Result<()> {
    let request = build_status_request(orb_id, cellular_info, wifi_networks)?;

    debug!(
        request = ?serde_json::to_string(&request)?,
        "Sending status v2 request"
    );

    let backend = Backend::from_env()?;

    // TODO: Some sort of actual response besides the status code
    // maybe one that includes the location? (requires changes on the
    // backend)
    let _response_raw = client()
        .post(EndpointsV2::new(backend, orb_id).status)
        .json(&request)
        .send()?
        .error_for_status()?;

    Ok(())
}

fn build_status_request(
    orb_id: &OrbId,
    cellular_info: &CellularInfo,
    wifi_networks: &[WifiNetwork],
) -> Result<OrbStatusV2> {
    let serving_cell = &cellular_info.serving_cell;

    let location_data = LocationDataV2 {
        wifi: Some(
            wifi_networks
                .iter()
                .map(|w| WifiDataV2 {
                    ssid: Some(w.ssid.clone()),
                    bssid: Some(w.bssid.clone()),
                    signal_strength: Some(w.signal_level),
                    channel: Some(w.frequency),
                    signal_to_noise_ratio: None,
                })
                .collect(),
        ),
        cell: Some(vec![CellDataV2 {
            mcc: serving_cell.mcc,
            mnc: serving_cell.mnc,
            lac: None,
            cell_id: Some(u32::from_str_radix(&serving_cell.cell_id, 16)?),
            signal_strength: serving_cell.rssi,
        }]),
    };

    Ok(OrbStatusV2 {
        orb_id: Some(orb_id.to_string()),
        location_data: Some(location_data),
    })
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;
    use crate::cell::data::ServingCell;
    use orb_endpoints::OrbId;

    #[test]
    fn test_build_status_request_valid() {
        let orb_id = OrbId::from_str("abcdef12").unwrap();

        let cellular_info = CellularInfo {
            serving_cell: ServingCell {
                connection_status: "CONNECT".to_string(),
                network_type: "LTE".to_string(),
                duplex_mode: "FDD".to_string(),
                mcc: Some(310),
                mnc: Some(260),
                cell_id: "00AB12".to_string(),
                channel_or_arfcn: Some(100),
                pcid_or_psc: Some(22),
                rsrp: Some(-90),
                rsrq: Some(-10),
                rssi: Some(-60),
                sinr: Some(12),
            },
            neighbor_cells: vec![],
        };

        let wifi_networks = vec![WifiNetwork {
            bssid: "00:11:22:33:44:55".into(),
            frequency: 2412,
            signal_level: -45,
            flags: "[WPA2-PSK-CCMP][ESS]".into(),
            ssid: "TestAP".into(),
        }];

        let request =
            build_status_request(&orb_id, &cellular_info, &wifi_networks).unwrap();
        assert_eq!(request.orb_id, Some("abcdef12".to_string()));

        let location_data = request
            .location_data
            .expect("Location data should be present");
        let cell_data = location_data.cell.expect("Cell data should be present");
        assert_eq!(cell_data.len(), 1);

        let cell = &cell_data[0];

        // 0x00AB12 => 43794 decimal
        assert_eq!(cell.cell_id, Some(0x00AB12));
        assert_eq!(cell.mcc, Some(310));
        assert_eq!(cell.mnc, Some(260));
        assert_eq!(cell.lac, None);

        let wifi_data = location_data.wifi.expect("WiFi data should be present");
        assert_eq!(wifi_data.len(), 1);
        let wifi = &wifi_data[0];

        assert_eq!(wifi.bssid, Some("00:11:22:33:44:55".to_string()));
        assert_eq!(wifi.ssid, Some("TestAP".to_string()));
        assert_eq!(wifi.signal_strength, Some(-45));
        assert_eq!(wifi.channel, Some(2412));
        assert_eq!(wifi.signal_to_noise_ratio, None);
    }

    #[test]
    fn test_build_status_request_invalid_hex() {
        let orb_id = OrbId::from_str("abcdef12").unwrap();

        let cellular_info = CellularInfo {
            serving_cell: ServingCell {
                connection_status: "CONNECT".to_string(),
                network_type: "LTE".to_string(),
                duplex_mode: "FDD".to_string(),
                mcc: Some(310),
                mnc: Some(260),
                cell_id: "GARBAGE".to_string(),
                channel_or_arfcn: None,
                pcid_or_psc: None,
                rsrp: None,
                rsrq: None,
                rssi: None,
                sinr: None,
            },
            neighbor_cells: vec![],
        };

        let wifi_networks = vec![];
        let err =
            build_status_request(&orb_id, &cellular_info, &wifi_networks).unwrap_err();

        assert!(err.to_string().contains("invalid digit"));
    }

    #[test]
    fn test_build_status_request_empty_networks() {
        let orb_id = OrbId::from_str("abcdef12").unwrap();

        let cellular_info = CellularInfo {
            serving_cell: ServingCell {
                connection_status: "CONNECT".to_string(),
                network_type: "LTE".to_string(),
                duplex_mode: "FDD".to_string(),
                mcc: Some(310),
                mnc: Some(260),
                cell_id: "1234".to_string(),
                channel_or_arfcn: None,
                pcid_or_psc: None,
                rsrp: None,
                rsrq: None,
                rssi: None,
                sinr: None,
            },
            neighbor_cells: vec![],
        };

        let wifi_networks = vec![];
        let request =
            build_status_request(&orb_id, &cellular_info, &wifi_networks).unwrap();

        let location_data = request
            .location_data
            .expect("Location data should be present");

        let wifi_data = location_data.wifi.expect("WiFi data should be present");
        assert!(wifi_data.is_empty());

        let cell_data = location_data.cell.expect("Cell data should be present");
        assert_eq!(cell_data.len(), 1);
    }
}
