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

use crate::data::WifiNetwork;

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

pub fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        let builder = orb_security_utils::reqwest::http_client_builder()
            .timeout(Duration::from_secs(60))
            .user_agent("orb-location");
        builder.build().expect("Failed to build client")
    })
}

/// Simplified function to send location status without cellular info
pub async fn send_location_status(
    wifi_networks: &[WifiNetwork],
    _unused: Option<()>,
) -> Result<String> {
    let orb_id = OrbId::read_blocking()?;
    send_location_data(&orb_id, wifi_networks).await
}

pub async fn send_location_data(
    orb_id: &OrbId,
    wifi_networks: &[WifiNetwork],
) -> Result<String> {
    let request = build_status_request_v2(orb_id, wifi_networks)?;

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

    debug!("Status update completed successfully");
    Ok(response_body)
}

fn build_status_request_v2(
    orb_id: &OrbId,
    wifi_networks: &[WifiNetwork],
) -> Result<OrbStatusV2> {
    // Build list of WiFi networks in the correct format
    let wifi_data = if !wifi_networks.is_empty() {
        let wifi_list = wifi_networks
            .iter()
            .map(|net| {
                // Try to convert frequency to channel
                let channel = freq_to_channel(net.frequency);

                WifiDataV2 {
                    ssid: Some(net.ssid.clone()),
                    bssid: Some(net.bssid.clone()),
                    signal_strength: Some(net.signal_level),
                    channel,
                    signal_to_noise_ratio: None, // Not available in current implementation
                }
            })
            .collect::<Vec<_>>();

        Some(wifi_list)
    } else {
        None
    };

    let location_data = LocationDataV2 { wifi: wifi_data };

    let request = OrbStatusV2 {
        orb_id: Some(orb_id.to_string()),
        location_data: Some(location_data),
        timestamp: Utc::now(),
    };

    Ok(request)
}

fn freq_to_channel(freq: u32) -> Option<u32> {
    match freq {
        2412 => Some(1),
        2417 => Some(2),
        2422 => Some(3),
        2427 => Some(4),
        2432 => Some(5),
        2437 => Some(6),
        2442 => Some(7),
        2447 => Some(8),
        2452 => Some(9),
        2457 => Some(10),
        2462 => Some(11),
        2467 => Some(12),
        2472 => Some(13),
        2484 => Some(14),
        // 5 GHz channels
        5170..=5825 => Some((freq - 5000) / 5),
        // 6 GHz channels (WiFi 6E)
        5955..=7115 => Some((freq - 5950) / 5),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_build_status_request_v2_wifi_only() {
        let orb_id = OrbId::from_str("test-orb-id").unwrap();
        let mut wifi_networks = Vec::new();

        wifi_networks.push(WifiNetwork {
            bssid: "00:11:22:33:44:55".to_string(),
            frequency: 2437,
            signal_level: -70,
            flags: "WPA2".to_string(),
            ssid: "TestNetwork".to_string(),
        });

        wifi_networks.push(WifiNetwork {
            bssid: "AA:BB:CC:DD:EE:FF".to_string(),
            frequency: 5220,
            signal_level: -80,
            flags: "WPA3".to_string(),
            ssid: "5GHzNetwork".to_string(),
        });

        let request = build_status_request_v2(&orb_id, &wifi_networks).unwrap();

        assert_eq!(request.orb_id, Some("test-orb-id".to_string()));
        assert!(request.location_data.is_some());

        let location_data = request.location_data.unwrap();
        assert!(location_data.wifi.is_some());

        let wifi_data = location_data.wifi.unwrap();
        assert_eq!(wifi_data.len(), 2);

        // Check first WiFi network
        assert_eq!(wifi_data[0].bssid, Some("00:11:22:33:44:55".to_string()));
        assert_eq!(wifi_data[0].ssid, Some("TestNetwork".to_string()));
        assert_eq!(wifi_data[0].signal_strength, Some(-70));
        assert_eq!(wifi_data[0].channel, Some(6));

        // Check second WiFi network
        assert_eq!(wifi_data[1].bssid, Some("AA:BB:CC:DD:EE:FF".to_string()));
        assert_eq!(wifi_data[1].ssid, Some("5GHzNetwork".to_string()));
        assert_eq!(wifi_data[1].signal_strength, Some(-80));
        assert_eq!(wifi_data[1].channel, Some(44));
    }

    #[test]
    fn test_freq_to_channel_conversion() {
        // 2.4 GHz band
        assert_eq!(freq_to_channel(2412), Some(1));
        assert_eq!(freq_to_channel(2437), Some(6));
        assert_eq!(freq_to_channel(2462), Some(11));

        // 5 GHz band
        assert_eq!(freq_to_channel(5180), Some(36));
        assert_eq!(freq_to_channel(5220), Some(44));
        assert_eq!(freq_to_channel(5745), Some(149));

        // 6 GHz band (WiFi 6E)
        assert_eq!(freq_to_channel(5975), Some(5));
        assert_eq!(freq_to_channel(6055), Some(21));

        // Invalid frequencies
        assert_eq!(freq_to_channel(1000), None);
        assert_eq!(freq_to_channel(3500), None);
        assert_eq!(freq_to_channel(8000), None);
    }
}
