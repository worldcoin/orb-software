use std::{path::Path, time::Duration};
use tracing::{debug, trace, warn};
use wpactrl::{Client, ClientAttached};

use eyre::{ensure, Context, Result};

const SCAN_TIMEOUT_SECS: u64 = 30;
const SCAN_POLL_INTERVAL_MS: u64 = 100;

#[derive(Debug)]
pub struct WifiNetwork {
    pub bssid: String,
    pub frequency: u32,
    pub signal_level: i32,
    pub flags: String,
    pub ssid: String,
}

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
        debug!("Initiating WiFi scan");
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

        let mut networks = Vec::new();
        for line in scan_results.lines().skip(1) {
            // skip header
            match parse_scan_result(line, self.filter_macs) {
                Ok(network) => networks.push(network),
                Err(e) if e.to_string().contains("invalid MAC address") => {
                    trace!("Skipping filtered MAC address in scan result: {}", line);
                }
                Err(e) => warn!("Failed to parse scan result line '{}': {}", line, e),
            }
        }

        debug!(network_count = networks.len(), "Parsed WiFi networks");
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

/// Filter location-irrelavent APs according to Google documentation
///
/// See https://developers.google.com/maps/documentation/geolocation/requests-geolocation#filter_mac
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
