use crate::ssh_wrapper::SshWrapper;
use clap::Parser;
use color_eyre::{
    eyre::{bail, WrapErr},
    Result,
};
use regex::Regex;
use serde_json::Value;
use std::path::PathBuf;
use tokio::io::AsyncWriteExt;
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

    /// Path to CI summary file for PR comments
    #[arg(long)]
    ci_summary_file: Option<PathBuf>,
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

        self.run_post_update_verification(&session, &current_slot, &current_version)
            .await?;

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

    fn parse_check_my_orb_output(&self, output: &str) -> Result<()> {
        let clean_output = Self::strip_ansi_sequences(output);
        let lines: Vec<&str> = clean_output.lines().collect();

        for line in lines {
            if line.contains("[ FAILURE ]") {
                error!("{}", line);
            } else if line.contains("[ WARNING ]") {
                warn!("{}", line);
            }
        }
        Ok(())
    }

    #[instrument(skip(self, session))]
    async fn run_post_update_verification(
        &self,
        session: &SshWrapper,
        current_slot: &str,
        current_version: &str,
    ) -> Result<()> {
        println!("\n=== POST-UPDATE VERIFICATION ===");

        self.append_ci_summary("\n## Post-Update Verification\n\n")
            .await?;

        println!("\n--- System Health Check ---");
        let check_my_orb_output = self.run_check_my_orb_and_capture(session).await?;
        self.append_ci_summary("### check-my-orb output\n\n```\n")
            .await?;
        self.append_ci_summary(&check_my_orb_output).await?;
        self.append_ci_summary("\n```\n\n").await?;

        println!("\n--- MCU Information ---");
        let mcu_info_output = self.print_orb_mcu_util_info_and_capture(session).await?;
        self.append_ci_summary("### mcu util output\n\n```\n")
            .await?;
        self.append_ci_summary(&mcu_info_output).await?;
        self.append_ci_summary("\n```\n\n").await?;

        println!("\n--- Summary ---");
        let capsule_status = self.check_capsule_update_status(session).await?;
        self.append_ci_summary("### Summary\n\n").await?;
        self.append_ci_summary(&format!("- **Capsule status:** {capsule_status}\n"))
            .await?;

        println!("\n--- Current Slot ---");
        println!("Current slot: {}", current_slot);
        self.append_ci_summary(&format!("- **Current slot:** {current_slot}\n"))
            .await?;

        if let Some(pre_slot) = self.parse_pre_update_slot_from_ci_summary().await? {
            println!("\n--- Slot Switch Verification ---");
            if current_slot != pre_slot {
                println!("✅ Slot switch verified: {pre_slot} -> {current_slot}");
                self.append_ci_summary(&format!(
                    "- **Slot switch:** ✅ VERIFIED - {pre_slot} -> {current_slot}\n"
                ))
                .await?;
            } else {
                println!("❌ Slot switch failed: still on {current_slot}");
                self.append_ci_summary(&format!(
                    "- **Slot switch:** ❌ FAILED - still on {current_slot}\n"
                ))
                .await?;
            }
        } else {
            println!("\n--- Slot Switch Verification ---");
            println!("ℹ️  Could not determine pre-update slot from CI summary file");
            self.append_ci_summary(
                "- **Slot switch:** ℹ️  Could not determine pre-update slot\n",
            )
            .await?;
        }

        println!("\n--- Version Verification ---");
        if current_version.starts_with("to-") {
            error!(
                "❌ Version still contains 'to-' prefix: {}",
                current_version
            );
            self.append_ci_summary(&format!("- **Version check:** ❌ FAILED - still has 'to-' prefix: {current_version}\n")).await?;
            bail!("Update may not have completed properly - version still has 'to-' prefix");
        } else {
            println!("✅ Version prefix check passed: {current_version}");
            self.append_ci_summary(&format!(
                "- **Version check:** ✅ PASSED - {current_version}\n"
            ))
            .await?;
        }

        println!("\n--- Release ID Verification ---");
        match self
            .verify_release_id_matches_version(session, current_version)
            .await
        {
            Ok(_) => {
                self.append_ci_summary("- **Release ID verification:** ✅ PASSED\n")
                    .await?;
            }
            Err(e) => {
                self.append_ci_summary(&format!(
                    "- **Release ID verification:** ❌ FAILED - {e}\n"
                ))
                .await?;
                return Err(e);
            }
        }

        println!("\n--- Internet Connection Check ---");
        match self.check_internet_connection(session).await {
            Ok(_) => {
                self.append_ci_summary("- **Internet connection:** ✅ PASSED\n")
                    .await?;
            }
            Err(e) => {
                self.append_ci_summary(&format!(
                    "- **Internet connection:** ❌ FAILED - {e}\n"
                ))
                .await?;
                return Err(e);
            }
        }

        println!("\n=== POST-UPDATE VERIFICATION COMPLETE ===\n");
        Ok(())
    }

    #[instrument(skip(self, session))]
    async fn run_check_my_orb_and_capture(
        &self,
        session: &SshWrapper,
    ) -> Result<String> {
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

        Ok(Self::strip_ansi_sequences(stdout))
    }

    #[instrument(skip(self, session))]
    async fn print_orb_mcu_util_info_and_capture(
        &self,
        session: &SshWrapper,
    ) -> Result<String> {
        let result = session
            .execute_command("orb-mcu-util info")
            .await
            .wrap_err("Failed to run orb-mcu-util info")?;

        if !result.is_success() {
            warn!("orb-mcu-util info failed: {}", result.stderr);
            return Ok("orb-mcu-util info failed".to_string());
        }

        println!("{}", result.stdout);

        Ok(Self::strip_ansi_sequences(&result.stdout))
    }

    #[instrument(skip(self, session))]
    async fn verify_release_id_matches_version(
        &self,
        session: &SshWrapper,
        current_version: &str,
    ) -> Result<()> {
        let result = session
            .execute_command("cat /etc/release-id")
            .await
            .wrap_err("Failed to read /etc/release-id")?;

        if !result.is_success() {
            bail!("Failed to read /etc/release-id: {}", result.stderr);
        }

        let release_id = result.stdout.trim();
        println!("Release ID: {release_id}");

        let version_prefix = current_version.split('-').next().unwrap_or("");

        if release_id == version_prefix {
            println!(
                "✅ Release ID matches version prefix: {release_id} == {version_prefix}"
            );
        } else {
            error!(
                "❌ Release ID mismatch: {} != {}",
                release_id, version_prefix
            );
            bail!("Release ID does not match version prefix");
        }

        Ok(())
    }

    #[instrument(skip(self, session))]
    async fn check_internet_connection(&self, session: &SshWrapper) -> Result<()> {
        let result = session
            .execute_command("ping -c 3 google.com")
            .await
            .wrap_err("Failed to ping google.com")?;

        if result.is_success() {
            println!("✅ Internet connection is working");
        } else {
            error!("❌ Internet connection failed: {}", result.stderr);
            bail!("Internet connection check failed");
        }

        Ok(())
    }

    async fn parse_pre_update_slot_from_ci_summary(&self) -> Result<Option<String>> {
        if let Some(summary_file) = &self.ci_summary_file {
            if summary_file.exists() {
                let content = tokio::fs::read_to_string(summary_file)
                    .await
                    .wrap_err("Failed to read CI summary file")?;

                for line in content.lines() {
                    if line.contains("**Current slot:**") {
                        if let Some(slot_start) = line.find("slot_") {
                            let slot_part = &line[slot_start..];
                            if let Some(slot_end) = slot_part.find(' ') {
                                let slot = &slot_part[..slot_end];
                                return Ok(Some(slot.to_string()));
                            } else {
                                let slot = slot_part.trim_end();
                                return Ok(Some(slot.to_string()));
                            }
                        }
                    }
                }
            }
        }
        Ok(None)
    }

    async fn append_ci_summary(&self, content: &str) -> Result<()> {
        if let Some(summary_file) = &self.ci_summary_file {
            tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(summary_file)
                .await
                .wrap_err("Failed to open CI summary file for appending")?
                .write_all(content.as_bytes())
                .await
                .wrap_err("Failed to append to CI summary file")?;
        }
        Ok(())
    }
}
