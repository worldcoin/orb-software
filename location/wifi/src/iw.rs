use std::{collections::HashMap, process::Command, str::FromStr, time::Duration};
use tracing::debug;

use crate::{is_valid_bssid, is_valid_mac, WifiNetwork};
use eyre::{eyre, Context, Result};

const SCAN_DELAY_MS: u64 = 1000;

// New struct for scanning using iw
pub struct IwScanner {
    interface: String,
    filter_macs: bool,
}

impl IwScanner {
    pub fn new(interface: &str, filter_macs: bool) -> Self {
        Self {
            interface: interface.to_string(),
            filter_macs,
        }
    }

    pub fn scan_wifi(&self) -> Result<Vec<WifiNetwork>> {
        // Default to a single scan
        self.scan_wifi_with_count(1)
    }

    pub fn scan_wifi_with_count(&self, scan_count: u32) -> Result<Vec<WifiNetwork>> {
        debug!(
            "Initiating WiFi scan using iw on {} (performing {} scans)",
            self.interface, scan_count
        );

        // Use a HashMap to deduplicate networks by BSSID and keep the strongest signal
        let mut networks_map: HashMap<String, WifiNetwork> = HashMap::new();

        // Perform multiple scans to get more complete results
        for i in 0..scan_count {
            debug!("Starting scan {} of {}", i + 1, scan_count);

            // Run iw scan command with sudo
            let output = Command::new("sudo")
                .args(["iw", "dev", &self.interface, "scan"])
                .output()
                .wrap_err("Failed to execute sudo iw scan command")?;

            if !output.status.success() {
                return Err(eyre!(
                    "iw scan command failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                ));
            }

            // Parse the output
            let scan_output = String::from_utf8_lossy(&output.stdout);
            let scan_networks = self.parse_iw_scan(&scan_output)?;

            // Add to map, keeping strongest signal
            for network in scan_networks {
                if let Some(existing) = networks_map.get(&network.bssid) {
                    if network.signal_level > existing.signal_level {
                        networks_map.insert(network.bssid.clone(), network);
                    }
                } else {
                    networks_map.insert(network.bssid.clone(), network);
                }
            }

            // Add delay between scans if not the last scan
            if i < scan_count - 1 {
                std::thread::sleep(Duration::from_millis(SCAN_DELAY_MS));
            }
        }

        let networks: Vec<WifiNetwork> = networks_map.into_values().collect();
        debug!(
            network_count = networks.len(),
            "Parsed WiFi networks from multiple scans"
        );
        Ok(networks)
    }

    fn parse_iw_scan(&self, scan_output: &str) -> Result<Vec<WifiNetwork>> {
        let mut networks = Vec::new();
        let mut current_network: Option<WifiNetwork> = None;

        // Each BSS section starts with "BSS"
        for line in scan_output.lines() {
            let line = line.trim();

            if let Some(bss_str) = line.strip_prefix("BSS ") {
                // If we were building a network, add it to our list if it looks valid
                if let Some(network) = current_network.take() {
                    if network.frequency > 0
                        && is_valid_bssid(&network.bssid)
                        && (!self.filter_macs || is_valid_mac(&network.bssid))
                    {
                        networks.push(network);
                    }
                }

                // Extract BSSID from "BSS xx:xx:xx:xx:xx:xx(on wlan0)" format
                let mut bssid_parts = bss_str.trim().split('(');
                if let Some(bssid) = bssid_parts.next() {
                    let bssid = bssid.trim().to_string();

                    // Skip invalid BSSIDs or non-MAC addresses
                    if is_valid_bssid(&bssid) {
                        current_network = Some(WifiNetwork {
                            bssid,
                            frequency: 0,
                            signal_level: 0,
                            flags: String::new(),
                            ssid: String::new(),
                        });
                    }
                }
            } else if let Some(network) = &mut current_network {
                // Update network properties based on the line
                if line.starts_with("freq: ") {
                    if let Some(freq_str) = line.strip_prefix("freq: ") {
                        if let Ok(freq) = u32::from_str(freq_str.trim()) {
                            network.frequency = freq;
                        }
                    }
                } else if line.starts_with("signal: ") {
                    if let Some(signal_str) = line.strip_prefix("signal: ") {
                        // Format is like "-76.00 dBm"
                        if let Some(signal_val) = signal_str.split_whitespace().next() {
                            if let Ok(signal) = f64::from_str(signal_val) {
                                network.signal_level = signal as i32;
                            }
                        }
                    }
                } else if line.starts_with("SSID: ") {
                    if let Some(ssid) = line.strip_prefix("SSID: ") {
                        network.ssid = ssid.trim().to_string();
                    }
                } else if line.starts_with("capability: ") {
                    if let Some(flags) = line.strip_prefix("capability: ") {
                        network.flags = flags.trim().to_string();
                    }
                } else if line.contains("RSN:") || line.contains("WPA:") {
                    // If WPA/WPA2 security information found, add to flags
                    if !network.flags.contains("WPA") {
                        let security_type =
                            if line.contains("RSN:") { "WPA2" } else { "WPA" };

                        // Add the security type to flags if not already present
                        if network.flags.is_empty() {
                            network.flags = format!("[{}]", security_type);
                        } else if !network
                            .flags
                            .contains(&format!("[{}]", security_type))
                        {
                            network.flags =
                                format!("{}[{}]", network.flags, security_type);
                        }
                    }
                }
            }
        }

        // Don't forget the last network
        if let Some(network) = current_network {
            if network.frequency > 0
                && is_valid_bssid(&network.bssid)
                && (!self.filter_macs || is_valid_mac(&network.bssid))
            {
                networks.push(network);
            }
        }

        // Final cleanup to remove any duplicate networks (by BSSID)
        let mut unique_networks = Vec::new();
        let mut seen_bssids = std::collections::HashSet::new();

        for network in networks {
            if !seen_bssids.contains(&network.bssid) {
                seen_bssids.insert(network.bssid.clone());
                unique_networks.push(network);
            }
        }

        Ok(unique_networks)
    }

    // Get the currently connected network information
    pub fn get_current_network(&self) -> Result<Option<WifiNetwork>> {
        debug!(
            "Getting current network info using iw on {}",
            self.interface
        );

        // Get link information
        let output = Command::new("sudo")
            .args(["iw", "dev", &self.interface, "link"])
            .output()
            .wrap_err("Failed to execute sudo iw link command")?;

        if !output.status.success() || output.stdout.is_empty() {
            // Either command failed or no connection
            return Ok(None);
        }

        let link_output = String::from_utf8_lossy(&output.stdout);

        // Extract information
        let mut bssid = None;
        let mut ssid = None;
        let mut freq = None;
        let mut signal = None;

        for line in link_output.lines() {
            let line = line.trim();

            if line.starts_with("Connected to ") {
                bssid = line.split_whitespace().nth(2).map(String::from);
            } else if line.contains("SSID: ") {
                ssid = line.split("SSID: ").nth(1).map(String::from);
            } else if line.contains("freq: ") {
                freq = line
                    .split("freq: ")
                    .nth(1)
                    .and_then(|s| u32::from_str(s).ok());
            } else if line.contains("signal: ") {
                signal = line
                    .split("signal: ")
                    .nth(1)
                    .and_then(|s| s.split_whitespace().next())
                    .and_then(|s| f64::from_str(s).ok())
                    .map(|f| f as i32);
            }
        }

        // Construct network if we have the required information
        if let (Some(bssid), Some(ssid)) = (bssid, ssid) {
            Ok(Some(WifiNetwork {
                bssid,
                frequency: freq.unwrap_or(0),
                signal_level: signal.unwrap_or(0),
                flags: String::new(), // We don't get flags from `iw link`
                ssid,
            }))
        } else {
            Ok(None)
        }
    }

    // Comprehensive scan including current network
    pub fn comprehensive_scan(&self) -> Result<Vec<WifiNetwork>> {
        // Default to a single scan for backwards compatibility
        self.comprehensive_scan_with_count(1)
    }

    // New method that accepts scan count
    pub fn comprehensive_scan_with_count(
        &self,
        scan_count: u32,
    ) -> Result<Vec<WifiNetwork>> {
        let mut networks = self.scan_wifi_with_count(scan_count)?;

        if let Ok(Some(current)) = self.get_current_network() {
            if !networks.iter().any(|n| n.bssid == current.bssid) {
                networks.push(current);
            }
        }

        Ok(networks)
    }
}
