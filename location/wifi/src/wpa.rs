use std::{path::Path, time::Duration};

use eyre::{Context as _, Result};
use orb_google_geolocation_api::support::WifiNetwork;
use tracing::{debug, trace, warn};
use wpactrl::{Client, ClientAttached};

use crate::{parse_scan_result, SCAN_POLL_INTERVAL_MS, SCAN_TIMEOUT_SECS};

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
