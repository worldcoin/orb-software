use crate::ssh_wrapper::SshWrapper;
use color_eyre::{
    eyre::{bail, WrapErr},
    Result,
};
use std::collections::HashSet;
use std::time::{Duration, Instant};
use tracing::{info, instrument, warn};

use super::Ota;

impl Ota {
    #[instrument(skip_all)]
    pub(super) async fn check_service_failed(
        &self,
        session: &SshWrapper,
    ) -> Result<bool> {
        let result = session
            .execute_command(
                "TERM=dumb sudo systemctl is-failed worldcoin-update-agent.service",
            )
            .await
            .wrap_err("Failed to check service status")?;

        Ok(result.exit_status == 0)
    }

    #[instrument(skip_all)]
    pub(super) async fn monitor_update_progress(
        &self,
        session: &SshWrapper,
        start_timestamp: &str,
    ) -> Result<()> {
        const MAX_WAIT_SECONDS: u64 = 7200;
        const POLL_INTERVAL: u64 = 3;

        info!("Starting monitoring of update progress");
        let start_time = Instant::now();
        let timeout = Duration::from_secs(MAX_WAIT_SECONDS);
        let mut seen_lines = HashSet::new();
        let mut consecutive_failures = 0;
        const MAX_CONSECUTIVE_FAILURES: u32 = 10;

        while start_time.elapsed() < timeout {
            match self.check_service_failed(session).await {
                Ok(true) => {
                    bail!("Update agent service failed - update installation failed");
                }
                Ok(false) => {
                    // Service is not failed, continue monitoring
                }
                Err(e) => {
                    warn!("Error checking service status: {}", e);
                }
            }

            match self
                .fetch_new_log_lines(session, &mut seen_lines, start_timestamp)
                .await
            {
                Ok(new_lines) => {
                    consecutive_failures = 0;
                    for line in new_lines {
                        println!("{}", line.trim());

                        // Only check for reboot message - this is the success signal
                        if line.contains("waiting 10 seconds before reboot to allow propagation to backend") {
                            info!("Reboot message detected: {}", line.trim());
                            return Ok(());
                        }
                    }
                }
                Err(e) => {
                    consecutive_failures += 1;
                    warn!(
                        "Error fetching log lines (attempt {}): {}",
                        consecutive_failures, e
                    );

                    if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                        bail!("Too many consecutive failures fetching update logs");
                    }
                }
            }

            tokio::time::sleep(Duration::from_secs(POLL_INTERVAL)).await;
        }

        bail!(
            "Timeout waiting for update completion within {} seconds",
            MAX_WAIT_SECONDS
        );
    }

    #[instrument(skip_all)]
    pub(super) async fn fetch_new_log_lines(
        &self,
        session: &SshWrapper,
        seen_lines: &mut HashSet<String>,
        start_timestamp: &str,
    ) -> Result<Vec<String>> {
        // Use --since to only fetch logs from current run, avoiding old failures
        let command = format!(
            "TERM=dumb sudo journalctl -u worldcoin-update-agent.service --no-pager --since '{start_timestamp}'"
        );

        let result = session
            .execute_command(&command)
            .await
            .wrap_err("Failed to fetch journalctl logs")?;

        if !result.is_success() {
            warn!("Failed to fetch journalctl logs: {}", result.stderr);
            return Ok(Vec::new());
        }

        let mut new_lines = Vec::new();

        for line in result.stdout.lines() {
            if line.trim().is_empty() {
                continue;
            }

            if seen_lines.insert(line.to_string()) {
                new_lines.push(line.to_string());
            }
        }

        Ok(new_lines)
    }
}
