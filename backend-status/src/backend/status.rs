use chrono::Utc;
use eyre::Result;
use orb_endpoints::{v2::Endpoints as EndpointsV2, Backend};
use orb_info::OrbId;
use reqwest::Url;
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware, Extension};
use reqwest_tracing::{OtelName, TracingMiddleware};
use std::{str::FromStr, time::Duration};
use tokio::sync::watch;
use tracing::{error, instrument};

use crate::{args::Args, dbus::intf_impl::CurrentStatus};

use super::types::{
    LocationDataV2, NetIntfV2, NetStatsV2, OrbStatusV2, UpdateProgressV2, WifiDataV2,
};

pub trait BackendStatusClientT: Send + Sync {
    async fn send_status(&self, current_status: &CurrentStatus) -> Result<()>;
}

#[derive(Debug, Clone)]
pub struct StatusClient {
    client: ClientWithMiddleware,
    orb_id: OrbId,
    endpoint: Url,
    auth_token: watch::Receiver<String>,
}

impl StatusClient {
    pub async fn new(
        args: &Args,
        orb_id: OrbId,
        token_receiver: watch::Receiver<String>,
    ) -> Result<Self> {
        let reqwest_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .user_agent("orb-backend-status")
            .build()
            .expect("Failed to build client");
        let name = orb_id.as_str().to_string().into();
        let client = ClientBuilder::new(reqwest_client)
            .with_init(Extension(OtelName(name)))
            .with(TracingMiddleware::default())
            .build();

        let backend = match Backend::from_str(&args.backend) {
            Ok(backend) => backend,
            Err(e) => {
                error!("Error getting backend configuration: {}", e);
                return Err(eyre::eyre!("Failed to initialize backend from environment: {}. Make sure ORB_BACKEND is set correctly.", e));
            }
        };

        let endpoint = match backend {
            Backend::Local => Url::parse(&format!(
                "http://{}/api/v2/orbs/{}/status",
                args.status_local_address.clone().unwrap(),
                orb_id
            ))
            .unwrap(),
            _ => EndpointsV2::new(backend, &orb_id).status,
        };

        Ok(Self {
            client,
            orb_id: orb_id.clone(),
            endpoint,
            auth_token: token_receiver,
        })
    }
}

impl BackendStatusClientT for StatusClient {
    #[instrument(skip(self, current_status))]
    async fn send_status(&self, current_status: &CurrentStatus) -> Result<()> {
        let request = build_status_request_v2(&self.orb_id, current_status)?;

        // Try to get auth token
        let auth_token = self.auth_token.borrow().clone();

        // Build request with optional authentication
        let request_builder = self
            .client
            .post(self.endpoint.clone())
            .json(&request)
            .basic_auth(self.orb_id.to_string(), Some(auth_token));

        let response = request_builder.send().await?;

        let status = response.status();
        if !status.is_success() {
            let response_body = response.text().await.unwrap_or_default();
            return Err(eyre::eyre!(
                "Backend status error: {} - {}",
                status,
                response_body
            ));
        }

        Ok(())
    }
}

fn build_status_request_v2(
    orb_id: &OrbId,
    current_status: &CurrentStatus,
) -> Result<OrbStatusV2> {
    Ok(OrbStatusV2 {
        orb_id: Some(orb_id.to_string()),
        location_data: current_status.wifi_networks.as_ref().map(|wifi_networks| {
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
        }),
        update_progress: current_status.update_progress.as_ref().map(
            |update_progress| UpdateProgressV2 {
                download_progress: update_progress.download_progress,
                processed_progress: update_progress.processed_progress,
                install_progress: update_progress.install_progress,
                total_progress: update_progress.total_progress,
                error: update_progress.error.clone(),
            },
        ),
        net_stats: current_status
            .net_stats
            .as_ref()
            .map(|net_stats| NetStatsV2 {
                interfaces: net_stats
                    .interfaces
                    .iter()
                    .map(|i| NetIntfV2 {
                        name: i.name.clone(),
                        tx_bytes: i.tx_bytes,
                        rx_bytes: i.rx_bytes,
                        tx_packets: i.tx_packets,
                        rx_packets: i.rx_packets,
                        tx_errors: i.tx_errors,
                        rx_errors: i.rx_errors,
                    })
                    .collect(),
            }),
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
    use orb_backend_status_dbus::types::WifiNetwork;
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
