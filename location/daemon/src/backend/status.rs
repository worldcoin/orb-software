use std::{
    sync::{Mutex, OnceLock},
    time::Duration,
};

use chrono::{DateTime, Utc};
use eyre::{eyre, Result};
use orb_endpoints::{v2::Endpoints as EndpointsV2, Backend};
use orb_info::OrbId;
use serde::{Deserialize, Serialize};
use tokio::sync::watch;
use tracing::{debug, warn};

use orb_google_geolocation_api::support::{CellularInfo, WifiNetwork};

// Constants for token handling
const TOKEN_RETRY_ATTEMPTS: u32 = 3;
const TOKEN_RETRY_DELAY: Duration = Duration::from_secs(2);

// Global token receiver storage
static TOKEN_RECEIVER: OnceLock<Mutex<Option<watch::Receiver<String>>>> =
    OnceLock::new();

// Initialize the token receiver storage
fn init_token_receiver() -> &'static Mutex<Option<watch::Receiver<String>>> {
    TOKEN_RECEIVER.get_or_init(|| Mutex::new(None))
}

/// Set the token receiver for authentication
///
/// This function stores a watch::Receiver that provides real-time access to authentication tokens.
/// The receiver is used by the backend status module when making authenticated requests.
///
/// Similar to the approach used in fleet-cmdr, this allows the system to always have access
/// to the latest token without requiring background tasks to update a static value.
///
/// # Arguments
///
/// * `receiver` - A watch::Receiver<String> that provides access to the current authentication token
pub fn set_token_receiver(receiver: watch::Receiver<String>) {
    let token_receiver = init_token_receiver();
    if let Ok(mut guard) = token_receiver.lock() {
        debug!("Setting token receiver");
        *guard = Some(receiver);
    } else {
        warn!("Failed to acquire lock to set token receiver");
    }
}

/// Retrieve the current authentication token from the token receiver
///
/// This function accesses the stored watch::Receiver and retrieves the current token value.
/// It's used internally by the status endpoint when making authenticated requests to the backend.
///
/// # Returns
///
/// * `Result<String>` - The current authentication token if available, or an error if no token is set
///
/// # Errors
///
/// Returns an error if no token receiver has been set or if the lock cannot be acquired
pub fn get_auth_token() -> Result<String> {
    let token_receiver = init_token_receiver();
    match token_receiver.lock() {
        Ok(guard) => {
            if let Some(receiver) = guard.as_ref() {
                Ok(receiver.borrow().clone())
            } else {
                Err(eyre!("No token receiver has been set"))
            }
        }
        Err(_) => Err(eyre!("Failed to acquire lock to read token receiver")),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OrbStatusV2 {
    pub orb_id: Option<String>,
    pub location_data: Option<LocationDataV2>,
    pub timestamp: DateTime<Utc>,
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

pub fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        let builder = orb_security_utils::reqwest::http_client_builder()
            .timeout(Duration::from_secs(60))
            .user_agent("orb-cellcom");
        builder.build().expect("Failed to build client")
    })
}

pub async fn send_location_data(
    orb_id: &OrbId,
    cellular_info: Option<&CellularInfo>,
    wifi_networks: &[WifiNetwork],
) -> Result<String> {
    let request = build_status_request_v2(orb_id, cellular_info, wifi_networks)?;

    debug!(
        request = ?serde_json::to_string(&request)?,
        "Sending status v2 request"
    );

    // Check for ORB_BACKEND environment variable first to provide a better error message
    if std::env::var("ORB_BACKEND").is_err() {
        return Err(eyre::eyre!("ORB_BACKEND environment variable is not set. Please set it to 'stage', 'prod', or 'dev', or use the --backend command-line argument."));
    }

    let backend = match Backend::from_env() {
        Ok(backend) => backend,
        Err(e) => {
            debug!("Error getting backend configuration: {}", e);
            return Err(eyre::eyre!("Failed to initialize backend from environment: {}. Make sure ORB_BACKEND is set correctly.", e));
        }
    };

    let endpoint = EndpointsV2::new(backend, orb_id).status;

    debug!("Sending request to endpoint: {}", endpoint);

    // Try to get auth token
    let auth_token = match get_auth_token() {
        Ok(token) => {
            debug!("Using authentication token for request");
            Some(token)
        }
        Err(e) => {
            warn!("No authentication token available: {}. Proceeding without authentication.", e);
            None
        }
    };

    // Build request with optional authentication
    let mut request_builder = client().post(endpoint).json(&request);

    // Add authentication if token is available
    if let Some(token) = auth_token {
        request_builder = request_builder.basic_auth(orb_id.to_string(), Some(token));
        debug!("Added authentication to request");
    }

    // Send the request with retries
    let mut response = None;
    let mut last_error = None;

    for attempt in 1..=TOKEN_RETRY_ATTEMPTS {
        match request_builder.try_clone().unwrap().send().await {
            Ok(resp) => {
                response = Some(resp);
                break;
            }
            Err(e) => {
                debug!(
                    "Network error on attempt {} while sending status request: {}",
                    attempt, e
                );
                last_error = Some(e);

                if attempt < TOKEN_RETRY_ATTEMPTS {
                    tokio::time::sleep(TOKEN_RETRY_DELAY).await;
                }
            }
        }
    }

    let response = match response {
        Some(resp) => resp,
        None => {
            let err = last_error.unwrap();
            debug!("All retry attempts failed: {}", err);
            return Err(err.into());
        }
    };

    // Get status code and log it before checking for errors
    let status = response.status();
    debug!(status_code = ?status, "Received HTTP status from backend");

    // Get response body, regardless of status code
    let response_body = match response.text().await {
        Ok(body) => {
            if body.is_empty() {
                debug!("Response body is empty");
            } else {
                debug!(body_length = body.len(), "Received response body");
            }
            body
        }
        Err(e) => {
            debug!("Error reading response body: {}", e);
            return Err(e.into());
        }
    };

    // Check if the status code indicates an error
    if !status.is_success() {
        debug!(
            status_code = ?status,
            response_body = ?response_body,
            "Error response from status endpoint"
        );

        // Check for authentication errors specifically
        if status.as_u16() == 401 || status.as_u16() == 403 {
            return Err(eyre::eyre!(
                "Authentication failed: {} - {}. Check that your auth token is valid.",
                status,
                response_body
            ));
        }

        return Err(eyre::eyre!(
            "Backend returned error status: {} - {}",
            status,
            response_body
        ));
    }

    debug!(
        status_code = ?status,
        body_length = response_body.len(),
        "Successful response from status endpoint"
    );

    Ok(response_body)
}

fn build_status_request_v2(
    orb_id: &OrbId,
    cellular_info: Option<&CellularInfo>,
    wifi_networks: &[WifiNetwork],
) -> Result<OrbStatusV2> {
    let location_data = if let Some(cell_info) = cellular_info {
        // We have cellular data, so include it
        LocationDataV2 {
            wifi: Some(
                wifi_networks
                    .iter()
                    .map(|w| WifiDataV2 {
                        ssid: Some(w.ssid.clone()),
                        bssid: Some(w.bssid.clone()),
                        signal_strength: Some(w.signal_level),
                        channel: freq_to_channel(w.frequency),
                        signal_to_noise_ratio: None,
                    })
                    .collect(),
            ),
            cell: Some(vec![CellDataV2 {
                mcc: cell_info.serving_cell.mcc,
                mnc: cell_info.serving_cell.mnc,
                lac: None,
                cell_id: u32::from_str_radix(&cell_info.serving_cell.cell_id, 16).ok(),
                signal_strength: cell_info.serving_cell.rssi,
            }]),
        }
    } else {
        // No cellular data, only wifi
        LocationDataV2 {
            wifi: Some(
                wifi_networks
                    .iter()
                    .map(|w| WifiDataV2 {
                        ssid: Some(w.ssid.clone()),
                        bssid: Some(w.bssid.clone()),
                        signal_strength: Some(w.signal_level),
                        channel: freq_to_channel(w.frequency),
                        signal_to_noise_ratio: None,
                    })
                    .collect(),
            ),
            cell: None,
        }
    };

    Ok(OrbStatusV2 {
        orb_id: Some(orb_id.to_string()),
        location_data: Some(location_data),
        timestamp: Utc::now(),
    })
}

// Helper function to convert frequency to channel number
fn freq_to_channel(freq: u32) -> Option<u32> {
    // For 2.4 GHz: channel = (freq - 2412) / 5 + 1
    if (2412..=2484).contains(&freq) {
        if freq == 2484 {
            // Special case for channel 14
            return Some(14);
        }
        return Some((freq - 2412) / 5 + 1);
    }

    // For 5 GHz: varies by region, but generally channel = (freq - 5000) / 5
    if (5170..=5825).contains(&freq) {
        return Some((freq - 5000) / 5);
    }

    // For 6 GHz (Wi-Fi 6E): channel = (freq - 5950) / 5 + 1
    if (5955..=7115).contains(&freq) {
        return Some((freq - 5950) / 5 + 1);
    }

    None
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;
    use orb_cellcom::data::ServingCell;
    use orb_info::OrbId;

    #[test]
    fn test_build_status_request_v2_with_cell() {
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
            build_status_request_v2(&orb_id, Some(&cellular_info), &wifi_networks)
                .unwrap();
        assert_eq!(request.orb_id, Some("abcdef12".to_string()));
        assert!(request.timestamp <= Utc::now());

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
        assert_eq!(wifi.channel, Some(1)); // 2412 MHz = channel 1
        assert_eq!(wifi.signal_to_noise_ratio, None);
    }

    #[test]
    fn test_build_status_request_v2_wifi_only() {
        let orb_id = OrbId::from_str("abcdef12").unwrap();

        let wifi_networks = vec![WifiNetwork {
            bssid: "00:11:22:33:44:55".into(),
            frequency: 2412,
            signal_level: -45,
            flags: "[WPA2-PSK-CCMP][ESS]".into(),
            ssid: "TestAP".into(),
        }];

        let request = build_status_request_v2(&orb_id, None, &wifi_networks).unwrap();

        assert_eq!(request.orb_id, Some("abcdef12".to_string()));
        assert!(request.timestamp <= Utc::now());

        let location_data = request
            .location_data
            .expect("Location data should be present");

        assert!(
            location_data.cell.is_none(),
            "Cell data should not be present"
        );

        let wifi_data = location_data.wifi.expect("WiFi data should be present");
        assert_eq!(wifi_data.len(), 1);
        let wifi = &wifi_data[0];

        assert_eq!(wifi.bssid, Some("00:11:22:33:44:55".to_string()));
        assert_eq!(wifi.ssid, Some("TestAP".to_string()));
        assert_eq!(wifi.signal_strength, Some(-45));
        assert_eq!(wifi.channel, Some(1)); // 2412 MHz = channel 1
        assert_eq!(wifi.signal_to_noise_ratio, None);
    }

    #[test]
    fn test_freq_to_channel_conversion() {
        // 2.4 GHz band
        assert_eq!(freq_to_channel(2412), Some(1));
        assert_eq!(freq_to_channel(2437), Some(6));
        assert_eq!(freq_to_channel(2472), Some(13));
        assert_eq!(freq_to_channel(2484), Some(14));

        // 5 GHz band
        assert_eq!(freq_to_channel(5180), Some(36));
        assert_eq!(freq_to_channel(5500), Some(100));

        // 6 GHz band
        assert_eq!(freq_to_channel(5955), Some(2));
        assert_eq!(freq_to_channel(6175), Some(46));

        // Invalid frequencies
        assert_eq!(freq_to_channel(1000), None);
        assert_eq!(freq_to_channel(9000), None);
    }
}
