use std::{path::Path, time::Duration, collections::HashMap, process::Command, str::FromStr};
use tracing::{debug, trace, warn};
use wpactrl::{Client, ClientAttached};

use crate::data::WifiNetwork;
use eyre::{ensure, Context, Result, eyre};

const SCAN_TIMEOUT_SECS: u64 = 30;
const SCAN_POLL_INTERVAL_MS: u64 = 100;
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

    pub fn scan_wifi_with_count(&mut self, scan_count: u32) -> Result<Vec<WifiNetwork>> {
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
                        trace!("Skipping filtered MAC address in scan result: {}", line);
                    }
                    Err(e) => warn!("Failed to parse scan result line '{}': {}", line, e),
                }
            }
            
            // Add delay between scans if not the last scan
            if i < scan_count - 1 {
                std::thread::sleep(Duration::from_millis(SCAN_DELAY_MS));
            }
        }

        // Convert the HashMap to a Vec
        let networks: Vec<WifiNetwork> = networks_map.into_values().collect();
        debug!(network_count = networks.len(), "Parsed WiFi networks from multiple scans");
        Ok(networks)
    }

    // Try to get the currently connected network, which might not appear in scan results
    pub fn get_current_network(&mut self) -> Result<Option<WifiNetwork>> {
        debug!("Checking for currently connected network");
        let status = self.ctrl
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

/// Check if a string is a valid BSSID (MAC address)
fn is_valid_bssid(bssid: &str) -> bool {
    // BSSID should be a valid MAC address format (xx:xx:xx:xx:xx:xx)
    if bssid.is_empty() || !bssid.contains(':') || bssid.contains("Load:") {
        return false;
    }
    
    // Should have 5 colons (6 pairs of hex digits)
    let colon_count = bssid.chars().filter(|&c| c == ':').count();
    if colon_count != 5 {
        return false;
    }
    
    // Each part should be a valid hex number
    for part in bssid.split(':') {
        if part.len() != 2 || u8::from_str_radix(part, 16).is_err() {
            return false;
        }
    }
    
    true
}

/// Filter location-irrelevant APs
///
/// Filters out broadcast addresses, locally-administered MACs, and IANA reserved ranges
fn is_valid_mac(mac: &str) -> bool {
    // Broadcast addresses
    if mac.to_uppercase() == "FF:FF:FF:FF:FF:FF" {
        trace!(group = "broadcast", "filtered {mac}");
        return false;
    }

    // Locally-administered MACs (second LSB is 1)
    if let Some(first_byte) = mac.split(':').next() {
        if let Ok(byte) = u8::from_str_radix(first_byte, 16) {
            if byte & 0x02 != 0 {
                trace!(group = "u/l", "filtered {mac}");
                return false;
            }
        }
    }

    // IANA reserved range (00:00:5E:00:00:00 to 00:00:5E:FF:FF:FF)
    if mac.to_uppercase().starts_with("00:00:5E") {
        trace!(group = "iana", "filtered {mac}");
        return false;
    }

    trace!("passed {mac}");
    true
}

pub fn parse_scan_result(line: &str, filter_macs: bool) -> Result<WifiNetwork> {
    let fields: Vec<&str> = line.split('\t').collect();

    ensure!(fields.len() >= 5, "invalid scan result format");

    let bssid = fields[0].to_string();
    if filter_macs {
        ensure!(is_valid_mac(&bssid), "invalid MAC address");
    }

    Ok(WifiNetwork {
        bssid,
        frequency: fields[1].parse()?,
        signal_level: fields[2].parse()?,
        flags: fields[3].to_string(),
        ssid: fields[4].to_string(),
    })
}

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
        debug!("Initiating WiFi scan using iw on {} (performing {} scans)", self.interface, scan_count);
        
        // Use a HashMap to deduplicate networks by BSSID and keep the strongest signal
        let mut networks_map: HashMap<String, WifiNetwork> = HashMap::new();
        
        // Perform multiple scans to get more complete results
        for i in 0..scan_count {
            debug!("Starting scan {} of {}", i + 1, scan_count);
            
            // Run iw scan command with sudo
            let output = Command::new("sudo")
                .args(&["iw", "dev", &self.interface, "scan"])
                .output()
                .wrap_err("Failed to execute sudo iw scan command")?;
            
            if !output.status.success() {
                return Err(eyre!("iw scan command failed: {}", String::from_utf8_lossy(&output.stderr)));
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
        debug!(network_count = networks.len(), "Parsed WiFi networks from multiple scans");
        Ok(networks)
    }
    
    fn parse_iw_scan(&self, scan_output: &str) -> Result<Vec<WifiNetwork>> {
        let mut networks = Vec::new();
        let mut current_network: Option<WifiNetwork> = None;
        
        // Each BSS section starts with "BSS"
        for line in scan_output.lines() {
            let line = line.trim();
            
            if line.starts_with("BSS ") {
                // If we were building a network, add it to our list if it looks valid
                if let Some(network) = current_network.take() {
                    if network.frequency > 0 && is_valid_bssid(&network.bssid) && 
                       (!self.filter_macs || is_valid_mac(&network.bssid)) {
                        networks.push(network);
                    }
                }
                
                // Extract BSSID from "BSS xx:xx:xx:xx:xx:xx(on wlan0)" format
                let mut bssid_parts = line[4..].trim().split('(');
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
                        let security_type = if line.contains("RSN:") { "WPA2" } else { "WPA" };
                        
                        // Add the security type to flags if not already present
                        if network.flags.is_empty() {
                            network.flags = format!("[{}]", security_type);
                        } else if !network.flags.contains(&format!("[{}]", security_type)) {
                            network.flags = format!("{}[{}]", network.flags, security_type);
                        }
                    }
                }
            }
        }
        
        // Don't forget the last network
        if let Some(network) = current_network {
            if network.frequency > 0 && is_valid_bssid(&network.bssid) && 
               (!self.filter_macs || is_valid_mac(&network.bssid)) {
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
        debug!("Getting current network info using iw on {}", self.interface);
        
        // Get link information
        let output = Command::new("sudo")
            .args(&["iw", "dev", &self.interface, "link"])
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
                freq = line.split("freq: ").nth(1)
                    .and_then(|s| u32::from_str(s).ok());
            } else if line.contains("signal: ") {
                signal = line.split("signal: ").nth(1)
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
    pub fn comprehensive_scan_with_count(&self, scan_count: u32) -> Result<Vec<WifiNetwork>> {
        let mut networks = self.scan_wifi_with_count(scan_count)?;
        
        if let Ok(Some(current)) = self.get_current_network() {
            if !networks.iter().any(|n| n.bssid == current.bssid) {
                networks.push(current);
            }
        }
        
        Ok(networks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mac_address_validation() {
        // Valid
        assert!(is_valid_mac("00:11:22:33:44:55"));
        assert!(is_valid_mac("AC:DE:48:00:11:22"));

        // Invalid
        assert!(!is_valid_mac("FF:FF:FF:FF:FF:FF")); // Broadcast
        assert!(!is_valid_mac("02:00:00:00:00:00")); // Locally administered
        assert!(!is_valid_mac("00:00:5E:00:00:00")); // IANA reserved
        assert!(!is_valid_mac("00:00:5E:FF:FF:FF")); // IANA reserved
    }

    #[test]
    fn test_parse_scan_result_valid() {
        let line = "00:11:22:33:44:55\t2412\t-45\t[WPA2-PSK-CCMP][ESS]\tMyWiFi";
        let network = parse_scan_result(line, true).unwrap();
        assert_eq!(network.bssid, "00:11:22:33:44:55");
        assert_eq!(network.frequency, 2412);
        assert_eq!(network.signal_level, -45);
        assert_eq!(network.flags, "[WPA2-PSK-CCMP][ESS]");
        assert_eq!(network.ssid, "MyWiFi");
    }

    // FAILING
    #[test]
    fn test_parse_scan_result_invalid() {
        let line = "invalid line";
        let result = parse_scan_result(line, true);
        assert!(result.is_err());
    }
}
