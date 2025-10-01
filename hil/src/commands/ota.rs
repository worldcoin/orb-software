use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::ssh_wrapper::SshWrapper;
use clap::Parser;
use color_eyre::{
    eyre::{bail, WrapErr},
    Result,
};
use regex::Regex;
use serde_json::Value;
use tracing::{debug, error, info, instrument, trace, warn};

/// Over-The-Air update command for the Orb
#[derive(Debug, Parser)]
pub struct Ota {
    /// Target version to update to
    #[arg(long)]
    version: String,

    /// Hostname of the Orb device
    #[arg(long, default_value = "orb-bba85baa.local")]
    host: String,

    /// Username
    #[arg(long, default_value = "worldcoin")]
    username: String,

    /// Password
    #[arg(long)]
    password: String,

    /// SSH port for the Orb device
    #[arg(long, default_value = "22")]
    port: u16,

    /// Platform type (diamond or pearl)
    #[arg(long, value_enum)]
    platform: Platform,

    /// Skip wipe_overlays step (useful if command is not available)
    #[arg(long)]
    skip_wipe_overlays: bool,

    /// Timeout for the entire OTA process in seconds
    #[arg(long, default_value = "7200")] // 2 hours by default
    timeout_secs: u64,

    /// Path to save journalctl logs from worldcoin-update-agent.service
    #[arg(long)]
    log_file: PathBuf,

    /// Maximum reconnection attempts after reboot
    #[arg(long, default_value = "50")]
    max_reconnect_attempts: u32,

    /// Sleep time between reconnection attempts in seconds
    #[arg(long, default_value = "5")]
    reconnect_sleep_secs: u64,
}

#[derive(Debug, Clone, clap::ValueEnum)]
enum Platform {
    Diamond,
    Pearl,
}

impl Ota {
    fn strip_ansi_sequences(text: &str) -> String {
        let ansi_regex = Regex::new(r"\x1b\[[0-9;]*m").unwrap();
        ansi_regex.replace_all(text, "").to_string()
    }

    #[instrument(skip(self))]
    pub async fn run(self) -> Result<()> {
        let start_time = Instant::now();
        info!("Starting OTA update to version: {}", self.version);

        let mut session = self.create_ssh_session().await?;

        session = self.handle_platform_specific_steps(session).await?;

        let current_slot = self.determine_current_slot(&session).await?;
        info!("Current slot detected: {}", current_slot);

        self.update_versions_json(&session, &current_slot).await?;

        self.restart_update_agent(&session).await?;

        info!("Starting update progress and service status monitoring");
        self.monitor_update_progress(&session).await?;

        session = self.wait_for_reboot_and_reconnect().await?;

        info!("Device successfully rebooted and reconnected - update application completed");

        self.run_update_verifier(&session).await?;

        self.fetch_update_agent_logs(&session, start_time).await?;

        info!("OTA update completed successfully!");
        Ok(())
    }

    async fn create_ssh_session(&self) -> Result<SshWrapper> {
        info!("Connecting to Orb device at {}:{}", self.host, self.port);

        let session = SshWrapper::connect_with_password(
            self.host.clone(),
            self.port,
            self.username.clone(),
            self.password.clone(),
        )
        .await
        .wrap_err("Failed to establish SSH connection to Orb device")?;

        info!("Successfully connected to Orb device");
        Ok(session)
    }

    #[instrument(skip(self, session))]
    async fn handle_platform_specific_steps(
        &self,
        session: SshWrapper,
    ) -> Result<SshWrapper> {
        match self.platform {
            Platform::Diamond => {
                if self.skip_wipe_overlays {
                    info!("Diamond platform detected - skipping wipe_overlays (--skip-wipe-overlays specified)");
                    Ok(session)
                } else {
                    info!("Diamond platform detected - wiping overlays");
                    self.wipe_overlays(&session).await?;
                    info!(
                        "Overlays wiped, rebooting device and waiting for reconnection"
                    );
                    self.reboot_and_wait().await
                }
            }
            Platform::Pearl => {
                info!("Pearl platform detected - no special pre-update steps required");
                Ok(session)
            }
        }
    }

    #[instrument(skip(self, session))]
    async fn determine_current_slot(&self, session: &SshWrapper) -> Result<String> {
        info!("Determining current slot");
        let result = session
            .execute_command("TERM=dumb orb-slot-ctrl -c")
            .await
            .wrap_err("Failed to execute orb-slot-ctrl -c")?;

        if !result.is_success() {
            bail!("orb-slot-ctrl -c failed: {}", result.stderr);
        }

        let slot_regex = Regex::new(r"(a|b)")?;

        if let Some(captures) = slot_regex.captures(&result.stdout) {
            let slot_letter = captures.get(1).unwrap().as_str();
            let slot_name = format!("slot_{slot_letter}");
            info!("Current slot: {}", slot_name);
            Ok(slot_name)
        } else {
            bail!("Could not parse current slot from: {}", result.stdout);
        }
    }

    #[instrument(skip(self, session))]
    async fn update_versions_json(
        &self,
        session: &SshWrapper,
        current_slot: &str,
    ) -> Result<()> {
        info!(
            "Updating /usr/persistent/versions.json for slot {}",
            current_slot
        );

        let result = session
            .execute_command("TERM=dumb cat /usr/persistent/versions.json")
            .await
            .wrap_err("Failed to read /usr/persistent/versions.json")?;

        if !result.is_success() {
            bail!("Failed to read versions.json: {}", result.stderr);
        }

        let versions_content = &result.stdout;
        let mut versions_data: Value = serde_json::from_str(versions_content)
            .wrap_err("Failed to parse versions.json")?;

        // Update the current slot with the target version (with "to-" prefix)
        let version_with_prefix = format!("to-{}", self.version);
        if let Some(releases) = versions_data.get_mut("releases") {
            if let Some(releases_obj) = releases.as_object_mut() {
                releases_obj.insert(
                    current_slot.to_string(),
                    Value::String(version_with_prefix.clone()),
                );
                info!(
                    "Updated {} to version: {}",
                    current_slot, version_with_prefix
                );
            } else {
                bail!("releases field is not an object in versions.json");
            }
        } else {
            bail!("releases field not found in versions.json");
        }

        let updated_json_str = serde_json::to_string_pretty(&versions_data)
            .wrap_err("Failed to serialize updated versions.json")?;

        let result = session
            .execute_command(&format!(
                "echo '{updated_json_str}' | sudo tee /usr/persistent/versions.json > /dev/null"
            ))
            .await
            .wrap_err("Failed to write updated versions.json")?;

        if !result.is_success() {
            bail!("Failed to write versions.json: {}", result.stderr);
        }

        info!("versions.json updated successfully");
        Ok(())
    }

    async fn wipe_overlays(&self, session: &SshWrapper) -> Result<()> {
        // Source bash_profile to load the wipe_overlays function, then execute it
        let result = session
            .execute_command("bash -c 'source ~/.bash_profile 2>/dev/null || true; source ~/.bashrc 2>/dev/null || true; wipe_overlays'")
            .await
            .wrap_err("Failed to execute wipe_overlays function")?;

        if !result.is_success() {
            bail!("wipe_overlays function failed: {}", result.stderr);
        }

        info!("Overlays wiped successfully");
        Ok(())
    }

    #[instrument(skip(self, session))]
    async fn restart_update_agent(&self, session: &SshWrapper) -> Result<()> {
        info!("Restarting worldcoin-update-agent.service");

        let result = session
            .execute_command(
                "TERM=dumb sudo systemctl restart worldcoin-update-agent.service",
            )
            .await
            .wrap_err("Failed to restart worldcoin-update-agent.service")?;

        if !result.is_success() {
            bail!(
                "Failed to restart worldcoin-update-agent.service: {}",
                result.stderr
            );
        }

        info!("worldcoin-update-agent.service restarted successfully");
        Ok(())
    }

    #[instrument(skip(self, session))]
    async fn monitor_update_progress(&self, session: &SshWrapper) -> Result<()> {
        const MAX_WAIT_SECONDS: u64 = 7200; // 2 hours for download/install
        const LOG_POLL_INTERVAL: u64 = 5; // 5 seconds between log polls

        info!("Monitoring update progress via journalctl");
        let start_time = Instant::now();
        let timeout = Duration::from_secs(MAX_WAIT_SECONDS);

        while start_time.elapsed() < timeout {
            match self.check_update_agent_logs_for_reboot(session).await {
                Ok(reboot_detected) => {
                    if reboot_detected {
                        info!("Reboot detected in update agent logs - device will reboot now");
                        return Ok(());
                    }
                }
                Err(e) => {
                    warn!("Error checking update agent logs: {}", e);
                }
            }

            tokio::time::sleep(Duration::from_secs(LOG_POLL_INTERVAL)).await;
        }

        bail!(
            "Timeout waiting for update completion within {} seconds",
            MAX_WAIT_SECONDS
        );
    }

    #[instrument(skip(self, session))]
    async fn check_update_agent_logs_for_reboot(&self, session: &SshWrapper) -> Result<bool> {
        let result = session
            .execute_command("TERM=dumb sudo journalctl -u worldcoin-update-agent.service --no-pager -n 20")
            .await
            .wrap_err("Failed to fetch recent update agent logs")?;

        if !result.is_success() {
            warn!("Failed to fetch update agent logs: {}", result.stderr);
            return Ok(false);
        }

        let logs = &result.stdout;
        
        // Log progress information (ignore progress percentages as requested)
        for line in logs.lines() {
            if line.contains("%") && line.contains("progress") {
                info!("Update progress: {}", line.trim());
            }
        }

        // Check for reboot message
        if logs.to_lowercase().contains("reboot") {
            info!("Reboot message detected in logs: {}", logs);
            return Ok(true);
        }

        Ok(false)
    }

    #[instrument(skip(self, session))]
    async fn check_update_agent_status(&self, session: &SshWrapper) -> Result<String> {
        let result = session
            .execute_command(
                "TERM=dumb sudo systemctl is-active worldcoin-update-agent.service",
            )
            .await
            .wrap_err("Failed to check update agent status")?;

        if !result.is_success() {
            bail!("Failed to check update agent status: {}", result.stderr);
        }

        Ok(result.stdout.trim().to_string())
    }


    #[instrument(skip(self))]
    async fn wait_for_reboot_and_reconnect(&self) -> Result<SshWrapper> {
        info!("Waiting for automatic reboot and device to come back online");
        
        // Wait for device to come back online after automatic reboot
        let start_time = Instant::now();
        let timeout = Duration::from_secs(900); // 15 minutes timeout for reboot and update application

        while start_time.elapsed() < timeout {
            tokio::time::sleep(Duration::from_secs(10)).await;

            match self.create_ssh_session().await {
                Ok(session) => match self.test_connection(&session).await {
                    Ok(_) => {
                        info!("Device is back online and responsive after automatic reboot");
                        return Ok(session);
                    }
                    Err(e) => {
                        debug!("Connection test failed: {}", e);
                    }
                },
                Err(e) => {
                    debug!("Device not yet available: {}", e);
                }
            }
        }

        bail!("Device did not come back online within {:?}", timeout);
    }

    #[instrument(skip(self))]
    async fn reboot_and_wait(&self) -> Result<SshWrapper> {
        info!("Rebooting Orb device and waiting for reconnection");

        let temp_session = SshWrapper::connect_with_password(
            self.host.clone(),
            self.port,
            self.username.clone(),
            self.password.clone(),
        )
        .await
        .wrap_err("Failed to establish SSH connection for reboot command")?;

        let _result = temp_session.execute_command("sudo reboot").await;
        info!("Reboot command sent to Orb device");

        info!("Waiting for device to reboot and come back online");
        let start_time = Instant::now();
        let timeout = Duration::from_secs(900); // 15 minutes timeout for reboot and update application

        while start_time.elapsed() < timeout {
            tokio::time::sleep(Duration::from_secs(10)).await;

            match self.create_ssh_session().await {
                Ok(session) => match self.test_connection(&session).await {
                    Ok(_) => {
                        info!("Device is back online and responsive after reboot");
                        return Ok(session);
                    }
                    Err(e) => {
                        debug!("Connection test failed: {}", e);
                    }
                },
                Err(e) => {
                    debug!("Device not yet available: {}", e);
                }
            }
        }

        bail!("Device did not come back online within {:?}", timeout);
    }

    #[instrument(skip(self, session))]
    async fn test_connection(&self, session: &SshWrapper) -> Result<()> {
        let result = session
            .execute_command("echo connection_test")
            .await
            .wrap_err("Failed to execute test command")?;

        if !result.is_success() {
            bail!("Test command failed");
        }

        if !result.stdout.contains("connection_test") {
            bail!("Connection test output unexpected: {}", result.stdout);
        }

        Ok(())
    }

    #[instrument(skip(self, session))]
    async fn run_update_verifier(&self, session: &SshWrapper) -> Result<()> {
        info!("Running orb-update-verifier");

        let result = session
            .execute_command("TERM=dumb sudo orb-update-verifier")
            .await
            .wrap_err("Failed to run orb-update-verifier")?;

        if !result.is_success() {
            bail!("orb-update-verifier failed: {}", result.stderr);
        }

        let stdout = &result.stdout;
        info!("orb-update-verifier succeeded: {}", stdout);
        Ok(())
    }

    #[instrument(skip(self, session))]
    async fn fetch_update_agent_logs(
        &self,
        session: &SshWrapper,
        start_time: Instant,
    ) -> Result<()> {
        info!("Fetching logs from worldcoin-update-agent.service");

        let status_result = session
            .execute_command(
                "TERM=dumb sudo systemctl is-active worldcoin-update-agent.service",
            )
            .await
            .wrap_err("Failed to check worldcoin-update-agent.service status")?;

        let status = status_result.stdout.trim();
        if status != "active" {
            error!("worldcoin-update-agent.service is not active: {}", status);

            let logs = self.fetch_service_logs(session, start_time).await?;
            let clean_logs = Self::strip_ansi_sequences(&logs);

            let error_log = format!(
                "SERVICE FAILED: worldcoin-update-agent.service status: {status}\n\n=== Service Logs ===\n{clean_logs}");
            tokio::fs::write(&self.log_file, error_log.as_bytes())
                .await
                .wrap_err("Failed to write error logs to file")?;

            println!("=== worldcoin-update-agent.service FAILED ===");
            println!("Service status: {status}");
            println!("=== Service Logs ===");
            println!("{clean_logs}");
            println!("=== End of logs ===");

            bail!(
                "worldcoin-update-agent.service failed with status: {}",
                status
            );
        }

        let logs = self.fetch_service_logs(session, start_time).await?;
        let clean_logs = Self::strip_ansi_sequences(&logs);

        tokio::fs::write(&self.log_file, clean_logs.as_bytes())
            .await
            .wrap_err("Failed to write logs to file")?;

        info!("Update agent logs saved to {:?}", self.log_file);

        println!("=== worldcoin-update-agent.service logs ===");
        println!("{}", clean_logs);
        println!("=== End of logs ===");

        Ok(())
    }

    #[instrument(skip(self, session))]
    async fn fetch_service_logs(
        &self,
        session: &SshWrapper,
        start_time: Instant,
    ) -> Result<String> {
        let result = session
            .execute_command("TERM=dumb sudo journalctl -u worldcoin-update-agent.service --no-pager")
            .await
            .wrap_err("Failed to fetch logs from worldcoin-update-agent.service")?;

        if !result.is_success() {
            error!("Failed to fetch update agent logs: {}", result.stderr);

            let error_log = format!(
                "Failed to fetch logs: {}\nStderr: {}",
                result.stderr, result.stderr
            );
            tokio::fs::write(&self.log_file, error_log.as_bytes())
                .await
                .wrap_err("Failed to write error logs to file")?;

            bail!("Failed to fetch update agent logs: {}", result.stderr);
        }

        Ok(result.stdout)
    }
}
