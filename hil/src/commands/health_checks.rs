use crate::ssh_wrapper::SshWrapper;
use clap::Parser;
use color_eyre::{
    eyre::{bail, WrapErr},
    Result,
};
use regex::Regex;
use serde_json::Value;
use std::path::PathBuf;
use tracing::{debug, error, info, instrument, warn};

/// Health checks command for the Orb
#[derive(Debug, Parser)]
pub struct HealthChecks {
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

    /// Path to save health check logs
    #[arg(long)]
    log_file: Option<PathBuf>,

    /// Wait for device to be ready after reboot
    #[arg(long)]
    wait_for_ready: bool,
}

impl HealthChecks {
    fn strip_ansi_sequences(text: &str) -> String {
        let ansi_regex = Regex::new(r"\x1b\[[0-9;]*m").unwrap();
        ansi_regex.replace_all(text, "").to_string()
    }

    #[instrument(skip(self))]
    pub async fn run(self) -> Result<()> {
        info!("Starting health checks");

        let session = self.create_ssh_session().await?;

        let current_slot = self.check_current_slot(&session).await?;

        let current_version =
            self.check_current_version(&session, &current_slot).await?;

        let orb_id = self.get_orb_id(&session).await?;

        let capsule_status = self.check_capsule_update_status(&session).await?;

        self.run_check_my_orb(&session).await?;

        info!(
            "Summary: Slot={}, Version={}, OrbID={}, CapsuleStatus={}",
            current_slot, current_version, orb_id, capsule_status
        );

        Ok(())
    }

    #[instrument(skip(self))]
    async fn create_ssh_session(&self) -> Result<SshWrapper> {
        info!("Connecting to Orb device at {}:{}", self.host, self.port);

        if self.wait_for_ready {
            info!("Waiting for device to be ready after reboot");
            self.wait_for_device_ready().await?;
        }

        let session = SshWrapper::connect_with_password(
            self.host.clone(),
            self.port,
            self.username.clone(),
            self.password.clone(),
        )
        .await?;

        info!("Successfully connected to Orb device");

        session.test_connection().await?;

        Ok(session)
    }

    #[instrument(skip(self))]
    async fn wait_for_device_ready(&self) -> Result<()> {
        const MAX_ATTEMPTS: u32 = 30;
        const RETRY_INTERVAL: u64 = 10;

        for attempt in 1..=MAX_ATTEMPTS {
            debug!(
                "Attempting to connect to device (attempt {}/{})",
                attempt, MAX_ATTEMPTS
            );

            match SshWrapper::connect_with_password(
                self.host.clone(),
                self.port,
                self.username.clone(),
                self.password.clone(),
            )
            .await
            {
                Ok(session) => match session.test_connection().await {
                    Ok(_) => {
                        info!("Device is ready and responsive (attempt {})", attempt);
                        return Ok(());
                    }
                    Err(e) => {
                        debug!("Connection test failed on attempt {}: {}", attempt, e);
                    }
                },
                Err(e) => {
                    debug!("SSH connection failed on attempt {}: {}", attempt, e);
                }
            }

            if attempt < MAX_ATTEMPTS {
                debug!("Waiting {} seconds before retry...", RETRY_INTERVAL);
                tokio::time::sleep(std::time::Duration::from_secs(RETRY_INTERVAL))
                    .await;
            }
        }

        bail!(
            "Device did not become ready after {} attempts",
            MAX_ATTEMPTS
        );
    }

    #[instrument(skip(self, session))]
    async fn check_current_slot(&self, session: &SshWrapper) -> Result<String> {
        info!("Checking current slot");
        let result = session
            .execute_command("TERM=dumb orb-slot-ctrl -c")
            .await
            .wrap_err("Failed to execute orb-slot-ctrl -c")?;

        if !result.is_success() {
            bail!("orb-slot-ctrl -c failed: {}", result.stderr);
        }

        let output_str = &result.stdout;
        let slot_regex = Regex::new(r"(a|b)")?;

        if let Some(captures) = slot_regex.captures(output_str) {
            let slot_letter = captures.get(1).unwrap().as_str();
            let slot_name = format!("slot_{slot_letter}");
            info!("Current slot: {}", slot_name);
            Ok(slot_name)
        } else {
            bail!("Could not parse current slot from: {}", output_str);
        }
    }

    #[instrument(skip(self, session))]
    async fn check_current_version(
        &self,
        session: &SshWrapper,
        current_slot: &str,
    ) -> Result<String> {
        info!("Checking current version in slot {}", current_slot);

        let result = session
            .execute_command("cat /usr/persistent/versions.json")
            .await
            .wrap_err("Failed to read /usr/persistent/versions.json")?;

        if !result.is_success() {
            bail!("Failed to read versions.json: {}", result.stderr);
        }

        let versions_content = &result.stdout;
        let versions_data: Value = serde_json::from_str(versions_content)
            .wrap_err("Failed to parse versions.json")?;

        if let Some(releases) = versions_data.get("releases") {
            if let Some(releases_obj) = releases.as_object() {
                if let Some(version) = releases_obj.get(current_slot) {
                    if let Some(version_str) = version.as_str() {
                        info!("Current version in {}: {}", current_slot, version_str);
                        Ok(version_str.to_string())
                    } else {
                        bail!("Version in {} is not a string", current_slot);
                    }
                } else {
                    bail!("Slot {} not found in releases", current_slot);
                }
            } else {
                bail!("releases field is not an object in versions.json");
            }
        } else {
            bail!("releases field not found in versions.json");
        }
    }

    #[instrument(skip(self, session))]
    async fn get_orb_id(&self, session: &SshWrapper) -> Result<String> {
        info!("Getting orb-id");
        let result = session
            .execute_command("TERM=dumb orb-id")
            .await
            .wrap_err("Failed to execute orb-id command")?;

        if !result.is_success() {
            bail!("orb-id command failed: {}", result.stderr);
        }

        let output_str = &result.stdout;

        let orb_id_regex = Regex::new(r"^([a-f0-9]+)")?;
        if let Some(captures) = orb_id_regex.captures(output_str) {
            let orb_id = captures.get(1).unwrap().as_str();
            info!("Orb ID: {}", orb_id);
            Ok(orb_id.to_string())
        } else {
            warn!("Could not parse orb-id from output: {}", output_str);
            Ok("unknown".to_string())
        }
    }

    #[instrument(skip(self, session))]
    async fn check_capsule_update_status(
        &self,
        session: &SshWrapper,
    ) -> Result<String> {
        info!("Checking capsule update status");
        let result = session
            .execute_command("TERM=dumb sudo nvbootctrl dump-slots-info")
            .await
            .wrap_err("Failed to execute nvbootctrl dump-slots-info")?;

        if !result.is_success() {
            bail!("nvbootctrl dump-slots-info failed: {}", result.stderr);
        }

        let output = &result.stdout;

        let capsule_regex = Regex::new(r"Capsule update status: (\d+)")?;
        if let Some(captures) = capsule_regex.captures(output) {
            let status = captures.get(1).unwrap().as_str();
            let status_text = if status == "0" {
                "No update pending"
            } else {
                "Update pending"
            };
            info!("Capsule update status: {} ({})", status, status_text);
            Ok(format!("{status} ({status_text})"))
        } else {
            warn!(
                "Could not parse capsule update status from output: {}",
                output
            );
            Ok("unknown".to_string())
        }
    }

    #[instrument(skip(self, session))]
    async fn run_check_my_orb(&self, session: &SshWrapper) -> Result<()> {
        info!("Running check-my-orb");

        let result = session
            .execute_command("TERM=dumb check-my-orb")
            .await
            .wrap_err("Failed to run check-my-orb")?;

        let stdout = &result.stdout;
        let stderr = &result.stderr;

        self.parse_check_my_orb_output(stdout)?;

        if !stderr.is_empty() {
            warn!("check-my-orb stderr:\n{}", stderr);
        }

        if let Some(log_file) = &self.log_file {
            let clean_stdout = Self::strip_ansi_sequences(stdout);
            let clean_stderr = Self::strip_ansi_sequences(stderr);
            let log_content = format!(
                "=== check-my-orb output ===\n{clean_stdout}\n\n=== stderr ===\n{clean_stderr}"
            );
            tokio::fs::write(log_file, log_content.as_bytes())
                .await
                .wrap_err("Failed to write check-my-orb logs to file")?;
            info!("check-my-orb logs saved to {:?}", log_file);
        }

        info!("check-my-orb completed (may have failures)");
        Ok(())
    }

    fn parse_check_my_orb_output(&self, output: &str) -> Result<()> {
        let clean_output = Self::strip_ansi_sequences(output);
        let lines: Vec<&str> = clean_output.lines().collect();

        // Only logs the Failure / Warning
        // But will store the whole output into the logfile
        for line in lines {
            if line.contains("[ FAILURE ]") {
                error!("{}", line);
            } else if line.contains("[ WARNING ]") {
                warn!("{}", line);
            }
        }
        Ok(())
    }
}
