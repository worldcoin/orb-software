use chrono::Utc;
use eyre::Result;
use orb_endpoints::{v2::Endpoints as EndpointsV2, Backend};
use orb_info::{OrbId, TokenTaskHandle};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware, Extension};
use reqwest_tracing::{OtelName, TracingMiddleware};
use std::{str::FromStr, sync::Arc, time::Duration};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};
use zbus::Connection;

use crate::{args::Args, dbus::CurrentStatus};

use super::models::{LocationDataV2, OrbStatusV2, WifiDataV2};

#[derive(Debug, Clone)]
pub struct StatusClient {
    client: ClientWithMiddleware,
    orb_id: OrbId,
    auth_token: watch::Receiver<String>,
    _token_task: Option<Arc<TokenTaskHandle>>,
}

impl StatusClient {
    pub async fn new(args: &Args, shutdown_token: CancellationToken) -> Result<Self> {
        let reqwest_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .user_agent("orb-backend-status")
            .build()
            .expect("Failed to build client");
        let client = ClientBuilder::new(reqwest_client)
            .with_init(Extension(OtelName(
                format!("orb_{}", args.orb_id.as_ref().unwrap()).into(),
            )))
            .with(TracingMiddleware::default())
            .build();

        // Get token from args or DBus
        let mut _token_task: Option<Arc<TokenTaskHandle>> = None;
        let auth_token = if let Some(token) = args.orb_token.clone() {
            let (_, receiver) = watch::channel(token);
            receiver
        } else {
            let connection = Connection::session().await?;
            _token_task = Some(Arc::new(
                TokenTaskHandle::spawn(&connection, &shutdown_token).await?,
            ));
            _token_task.as_ref().unwrap().token_recv.to_owned()
        };

        let orb_id = OrbId::from_str(args.orb_id.as_ref().unwrap())?;

        Ok(Self {
            client,
            orb_id,
            auth_token,
            _token_task,
        })
    }

    pub async fn send_status(&self, current_status: &CurrentStatus) -> Result<String> {
        let request = build_status_request_v2(&self.orb_id, current_status)?;

        // Check for ORB_BACKEND environment variable first to provide a better error message
        let backend = match Backend::from_env() {
            Ok(backend) => backend,
            Err(e) => {
                error!("Error getting backend configuration: {}", e);
                return Err(eyre::eyre!("Failed to initialize backend from environment: {}. Make sure ORB_BACKEND is set correctly.", e));
            }
        };

        let endpoint = EndpointsV2::new(backend, &self.orb_id).status;

        // Try to get auth token
        let auth_token = self.auth_token.borrow().clone();

        // Build request with optional authentication
        let request_builder = self
            .client
            .post(endpoint)
            .body(serde_json::to_string(&request).unwrap())
            .header("Content-Type", "application/json")
            .basic_auth(self.orb_id.to_string(), Some(auth_token));

        // Send the request with retries
        let mut response = None;
        let mut last_error = None;

        match request_builder.try_clone().unwrap().send().await {
            Ok(resp) => {
                response = Some(resp);
            }
            Err(e) => {
                error!("Network error while sending status request: {}", e);
                last_error = Some(e);
            }
        }

        let response = match response {
            Some(resp) => resp,
            None => {
                let err = last_error.unwrap();
                error!("All retry attempts failed: {}", err);
                return Err(err.into());
            }
        };

        // Get status code and log it before checking for errors
        let status = response.status();
        info!(status_code = ?status, "Received HTTP status from backend");

        // Get response body, regardless of status code
        let response_body = match response.text().await {
            Ok(body) => {
                if body.is_empty() {
                    info!("Response body is empty");
                } else {
                    info!(body_length = body.len(), "Received response body");
                }
                body
            }
            Err(e) => {
                info!("Error reading response body: {}", e);
                return Err(e.into());
            }
        };

        // Check if the status code indicates an error
        if !status.is_success() {
            error!(
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

        info!(
            status_code = ?status,
            body_length = response_body.len(),
            "Successful response from status endpoint"
        );

        Ok(response_body)
    }
}

fn build_status_request_v2(
    orb_id: &OrbId,
    current_status: &CurrentStatus,
) -> Result<OrbStatusV2> {
    let location_data = LocationDataV2 {
        wifi: current_status.wifi_networks.as_ref().map(|wifi_networks| {
            wifi_networks
                .iter()
                .map(|w| WifiDataV2 {
                    ssid: Some(w.ssid.clone()),
                    bssid: Some(w.bssid.clone()),
                    signal_strength: Some(w.signal_level),
                    channel: freq_to_channel(w.frequency),
                    signal_to_noise_ratio: None,
                })
                .collect()
        }),
        cell: None,
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
    use orb_backend_status_dbus::WifiNetwork;
    use orb_info::OrbId;

    #[test]
    fn test_build_status_request_v2() {
        let orb_id = OrbId::from_str("abcdef12").unwrap();

        let wifi_networks = vec![WifiNetwork {
            bssid: "00:11:22:33:44:55".into(),
            frequency: 2412,
            signal_level: -45,
            flags: "[WPA2-PSK-CCMP][ESS]".into(),
            ssid: "TestAP".into(),
        }];

        let request = build_status_request_v2(
            &orb_id,
            &CurrentStatus {
                wifi_networks: Some(wifi_networks),
                ..Default::default()
            },
        )
        .unwrap();

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
