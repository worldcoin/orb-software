use tracing::trace;

use eyre::{ensure, Result};
use orb_google_geolocation_api::support::WifiNetwork;

const SCAN_TIMEOUT_SECS: u64 = 30;
const SCAN_POLL_INTERVAL_MS: u64 = 100;

pub mod iw;
pub mod wpa;

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
