use std::sync::LazyLock;

use crate::utils::run_cmd;

use super::{AccessPoint, WpaCtrl};
use async_trait::async_trait;
use color_eyre::Result;
use regex::Regex;

pub struct WpaCli;

impl WpaCli {
    const PATH: &str = "/usr/sbin/wpa_cli";
}

#[async_trait]
impl WpaCtrl for WpaCli {
    async fn scan_results(&self) -> Result<Vec<AccessPoint>> {
        let output = run_cmd(WpaCli::PATH, &["-i", "wlan0", "scan_results"]).await?;
        Ok(parse_scan_results(&output))
    }
}

fn parse_scan_results(str: &str) -> Vec<AccessPoint> {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?m)^(?P<bssid>(?:[0-9A-Fa-f]{2}:){5}[0-9A-Fa-f]{2})[ \t]+\d+[ \t]+(?P<rssi>-?\d+)[ \t]+(?:\[[^\]]*\])+(?:[ \t]+(?P<ssid>[^\n]+))?$").unwrap()
    });

    RE.captures_iter(str.trim())
        .filter_map(|caps| {
            Some(AccessPoint {
                bssid: caps.name("bssid")?.as_str().to_lowercase(),
                rssi: caps.name("rssi")?.as_str().parse().ok()?,
                ssid: caps.name("ssid").map(|m| m.as_str().trim().to_string()),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::parse_scan_results;
    use crate::wpa_ctrl::AccessPoint;

    #[test]
    fn it_parses_scan_results() {
        // Arrange
        let data = "bssid / frequency / signal level / flags / ssid
d6:80:94:c1:25:31       5660    -48     [WPA2-PSK+SAE-CCMP][SAE-H2E][ESS]       TFHOrbs
e6:80:94:c1:25:31       5660    -48     [WPA2-PSK+SAE-CCMP][SAE-H2E][ESS]       Tools for Humanity
a6:80:94:c1:25:37       5580    -70     [WPA2-PSK+SAE-CCMP][SAE-H2E][ESS]       Tools for Humanity-Guest
d6:80:94:c1:25:49       5580    -74     [WPA2-PSK+SAE-CCMP][SAE-H2E][ESS]       TFHOrbs
8a:78:48:d8:54:49       2437    -81     [ESS]   nonplusultra-guest
92:78:48:d8:54:4a       5300    -90     [WPA2-PSK-CCMP][ESS]
84:78:48:d8:54:4a       5300    -89     [WPA2-PSK+SAE-CCMP][ESS]        nonplusultra";

        // Act
        let actual = parse_scan_results(data);

        // Assert
        let expected = vec![
            AccessPoint {
                bssid: "d6:80:94:c1:25:31".into(),
                ssid: Some("TFHOrbs".into()),
                rssi: -48,
            },
            AccessPoint {
                bssid: "e6:80:94:c1:25:31".into(),
                ssid: Some("Tools for Humanity".into()),
                rssi: -48,
            },
            AccessPoint {
                bssid: "a6:80:94:c1:25:37".into(),
                ssid: Some("Tools for Humanity-Guest".into()),
                rssi: -70,
            },
            AccessPoint {
                bssid: "d6:80:94:c1:25:49".into(),
                ssid: Some("TFHOrbs".into()),
                rssi: -74,
            },
            AccessPoint {
                bssid: "8a:78:48:d8:54:49".into(),
                ssid: Some("nonplusultra-guest".into()),
                rssi: -81,
            },
            AccessPoint {
                bssid: "92:78:48:d8:54:4a".into(),
                ssid: None,
                rssi: -90,
            },
            AccessPoint {
                bssid: "84:78:48:d8:54:4a".into(),
                ssid: Some("nonplusultra".into()),
                rssi: -89,
            },
        ];

        assert_eq!(actual, expected);
    }
}
