use std::{collections::HashMap, path::Path, time::Duration};

use eyre::{Context as _, Result};
use orb_google_geolocation_api::support::WifiNetwork;
use tracing::{debug, trace, warn};
use wpactrl::{Client, ClientAttached};

use crate::{parse_scan_result, SCAN_POLL_INTERVAL_MS, SCAN_TIMEOUT_SECS};

const DEFAULT_SCAN_COUNT: u32 = 3;
const SCAN_DELAY_MS: u64 = 1000;

pub struct WpaSupplicant {
    ctrl: ClientAttached,
    filter_macs: bool,
}

impl WpaSupplicant {
    pub fn new(ctrl_path: &Path, filter_macs: bool) -> Result<Self> {
        let ctrl = Client::builder().ctrl_path(ctrl_path).open()?.attach()?;
        Ok(Self { ctrl, filter_macs })
    }

    pub fn scan_wifi(&mut self) -> Result<Vec<WifiNetwork>> {
        self.scan_wifi_with_count(DEFAULT_SCAN_COUNT)
    }

    pub fn scan_wifi_with_count(
        &mut self,
        scan_count: u32,
    ) -> Result<Vec<WifiNetwork>> {
        debug!("Initiating WiFi scan sequence ({} scans)", scan_count);

        // Use a HashMap to deduplicate networks by BSSID
        let mut networks_map: HashMap<String, WifiNetwork> = HashMap::new();

        // Perform multiple scans to get more complete results
        for i in 0..scan_count {
            debug!("Starting scan {} of {}", i + 1, scan_count);

            // Perform the scan
            self.ctrl
                .request("SCAN")
                .wrap_err("Failed to initiate scan")?;

            debug!("Waiting for scan results");
            self.wait_for_event(
                "CTRL-EVENT-SCAN-RESULTS",
                Duration::from_secs(SCAN_TIMEOUT_SECS),
            )?;

            debug!("Fetching scan results");
            let scan_results = self
                .ctrl
                .request("SCAN_RESULTS")
                .wrap_err("Failed to get scan results")?;

            // Process the scan results
            for line in scan_results.lines().skip(1) {
                // skip header
                match parse_scan_result(line, self.filter_macs) {
                    Ok(network) => {
                        // Merge networks, keeping the one with the strongest signal
                        if let Some(existing) = networks_map.get(&network.bssid) {
                            if network.signal_level > existing.signal_level {
                                networks_map.insert(network.bssid.clone(), network);
                            }
                        } else {
                            networks_map.insert(network.bssid.clone(), network);
                        }
                    }
                    Err(e) if e.to_string().contains("invalid MAC address") => {
                        trace!(
                            "Skipping filtered MAC address in scan result: {}",
                            line
                        );
                    }
                    Err(e) => {
                        warn!("Failed to parse scan result line '{}': {}", line, e)
                    }
                }
            }

            // Add delay between scans if not the last scan
            if i < scan_count - 1 {
                std::thread::sleep(Duration::from_millis(SCAN_DELAY_MS));
            }
        }

        // Convert the HashMap to a Vec
        let networks: Vec<WifiNetwork> = networks_map.into_values().collect();
        debug!(
            network_count = networks.len(),
            "Parsed WiFi networks from multiple scans"
        );
        Ok(networks)
    }

    // Try to get the currently connected network, which might not appear in scan results
    pub fn get_current_network(&mut self) -> Result<Option<WifiNetwork>> {
        debug!("Checking for currently connected network");
        let status = self
            .ctrl
            .request("STATUS")
            .wrap_err("Failed to get status")?;

        // Extract BSSID, SSID, and frequency from STATUS output
        let mut bssid = None;
        let mut ssid = None;
        let mut freq = None;
        let mut signal_level = None;

        for line in status.lines() {
            if line.starts_with("bssid=") {
                bssid = Some(line.trim_start_matches("bssid=").to_string());
            } else if line.starts_with("ssid=") {
                ssid = Some(line.trim_start_matches("ssid=").to_string());
            } else if line.starts_with("freq=") {
                if let Ok(f) = line.trim_start_matches("freq=").parse() {
                    freq = Some(f);
                }
            }
        }

        // If we found a BSSID and SSID, try to get the signal level
        if let (Some(bssid), Some(ssid)) = (bssid.as_ref(), ssid.as_ref()) {
            // Try to get signal level using SIGNAL_POLL
            if let Ok(signal_poll) = self.ctrl.request("SIGNAL_POLL") {
                for line in signal_poll.lines() {
                    if line.starts_with("RSSI=") {
                        if let Ok(rssi) = line.trim_start_matches("RSSI=").parse() {
                            signal_level = Some(rssi);
                            break;
                        }
                    }
                }
            }

            if freq.is_some() {
                return Ok(Some(WifiNetwork {
                    bssid: bssid.clone(),
                    frequency: freq.unwrap_or(0),
                    signal_level: signal_level.unwrap_or(0),
                    flags: String::new(), // We don't know the flags from STATUS
                    ssid: ssid.clone(),
                }));
            }
        }

        Ok(None)
    }

    // Method to get comprehensive scan results including current network
    pub fn comprehensive_scan(&mut self, scan_count: u32) -> Result<Vec<WifiNetwork>> {
        // Get scan results with multiple scans
        let mut networks = self.scan_wifi_with_count(scan_count)?;

        // Add the current network if it exists and is not already in the list
        if let Ok(Some(current)) = self.get_current_network() {
            if !networks.iter().any(|n| n.bssid == current.bssid) {
                networks.push(current);
            }
        }

        Ok(networks)
    }

    fn wait_for_event(
        &mut self,
        event_type: &str,
        timeout: Duration,
    ) -> Result<Option<String>> {
        let start_time = std::time::Instant::now();

        while start_time.elapsed() < timeout {
            if let Some(msg) = self.ctrl.recv()? {
                if msg.contains(event_type) {
                    return Ok(Some(msg));
                }
            }

            std::thread::sleep(Duration::from_millis(SCAN_POLL_INTERVAL_MS));
        }

        Ok(None)
    }
}
