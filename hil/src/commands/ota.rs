use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::serial::{spawn_serial_reader_task, LOGIN_PROMPT_PATTERN};
use crate::ssh_wrapper::{AuthMethod, SshConnectArgs, SshWrapper};
use clap::Parser;
use color_eyre::{
    eyre::{bail, ensure, eyre, WrapErr},
    Result,
};
use futures::StreamExt;
use serde_json::Value;
use tokio::sync::broadcast;
use tokio_serial::SerialPortBuilderExt;
use tokio_stream::wrappers::BroadcastStream;
use tracing::{debug, error, info, instrument, warn};

/// Over-The-Air update command for the Orb
#[derive(Debug, Parser)]
#[command(
    group = clap::ArgGroup::new("serial").required(true).multiple(false),
    group = clap::ArgGroup::new("auth").required(true).multiple(false)
)]
pub struct Ota {
    /// Target version to update to
    #[arg(long)]
    target_version: String,

    /// Hostname of the Orb device
    #[arg(long)]
    hostname: String,

    /// Username
    #[arg(long, default_value = "worldcoin")]
    username: String,

    /// Password for authentication (mutually exclusive with --key-path)
    #[arg(long, group = "auth")]
    password: Option<String>,

    /// Path to SSH private key for authentication (mutually exclusive with --password)
    #[arg(long, group = "auth")]
    key_path: Option<PathBuf>,

    /// SSH port for the Orb device
    #[arg(long, default_value = "22")]
    port: u16,

    /// Platform type (diamond or pearl)
    #[arg(long, value_enum)]
    platform: Platform,

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

    /// Serial port path for boot log capture
    #[arg(long, group = "serial")]
    serial_path: Option<PathBuf>,

    /// Serial port ID for boot log capture (alternative to --serial-path)
    #[arg(long, group = "serial")]
    serial_id: Option<String>,
}

#[derive(Debug, Clone, clap::ValueEnum)]
enum Platform {
    Diamond,
    Pearl,
}

impl Ota {
    /// Get the serial port path, either from --serial-path or --serial-id
    fn get_serial_path(&self) -> Result<PathBuf> {
        if let Some(ref serial_path) = self.serial_path {
            Ok(serial_path.clone())
        } else if let Some(ref serial_id) = self.serial_id {
            Ok(PathBuf::from(format!("/dev/serial/by-id/{serial_id}")))
        } else {
            bail!("Either --serial-path or --serial-id must be specified")
        }
    }

    #[instrument]
    pub async fn run(self) -> Result<()> {
        let _start_time = Instant::now();
        info!("Starting OTA update to version: {}", self.target_version);

        let session = match self.connect().await {
            Ok(session) => session,
            Err(e) => {
                println!("OTA_RESULT=FAILED");
                println!("OTA_ERROR=SSH_CONNECTION_FAILED: {e}");
                return Err(e);
            }
        };

        let (session, wipe_overlays_status) = match self.platform {
            Platform::Diamond => {
                info!("Diamond platform detected - wiping overlays before update");
                match self.wipe_overlays(&session).await {
                    Ok(_) => {
                        info!("Overlays wiped successfully, rebooting device");

                        let _result = session.execute_command("sudo reboot").await;
                        info!("Reboot command sent to Orb device");

                        // Pass the boot log prefix to (handle_reboot)
                        match self.handle_reboot("wipe_overlays").await {
                            Ok(new_session) => {
                                Ok((new_session, "succeeded".to_string()))
                            }
                            Err(e) => {
                                error!("Failed to reboot and reconnect after wiping overlays: {}", e);
                                Err(e)
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to wipe overlays: {}", e);
                        Err(e)
                    }
                }
            }
            Platform::Pearl => {
                info!("Pearl platform detected - no special pre-update steps required");
                Ok((session, "not_applicable".to_string()))
            }
        }?;

        let current_slot = match self.get_current_slot(&session).await {
            Ok(slot) => {
                info!("Current slot detected: {}", slot);
                slot
            }
            Err(e) => {
                println!("OTA_RESULT=FAILED");
                println!("OTA_ERROR=SLOT_DETECTION_FAILED: {e}");
                return Err(e);
            }
        };

        println!("OTA_SLOT={}", current_slot);
        println!("OTA_WIPE_OVERLAYS={}", wipe_overlays_status);

        if let Err(e) = self.update_versions_json(&session, &current_slot).await {
            println!("OTA_RESULT=FAILED");
            println!("OTA_ERROR=VERSION_UPDATE_FAILED: {e}");
            return Err(e);
        }

        if let Err(e) = self.restart_update_agent(&session).await {
            println!("OTA_RESULT=FAILED");
            println!("OTA_ERROR=UPDATE_AGENT_RESTART_FAILED: {e}");
            return Err(e);
        }

        info!("Starting update progress and service status monitoring");
        if let Err(e) = self.monitor_update_progress(&session).await {
            println!("OTA_RESULT=FAILED");
            println!("OTA_ERROR=UPDATE_PROGRESS_FAILED: {e}");
            return Err(e);
        }

        let session = match self.handle_reboot("update").await {
            Ok(session) => {
                info!("Device successfully rebooted and reconnected - update application completed");
                session
            }
            Err(e) => {
                println!("OTA_RESULT=FAILED");
                println!("OTA_ERROR=POST_UPDATE_REBOOT_FAILED: {e}");
                return Err(e);
            }
        };

        if let Err(e) = self.run_update_verifier(&session).await {
            println!("OTA_RESULT=FAILED");
            println!("OTA_ERROR=UPDATE_VERIFIER_FAILED: {e}");
            return Err(e);
        }

        if let Err(e) = self.get_capsule_update_status(&session).await {
            println!("OTA_RESULT=FAILED");
            println!("OTA_ERROR=CAPSULE_UPDATE_STATUS_FAILED: {e}");
            return Err(e);
        }

        if let Err(e) = self.run_check_my_orb(&session).await {
            println!("CHECK_MY_ORB_EXECUTION_FAILED: {e}");
        }

        if let Err(e) = self.get_boot_time(&session).await {
            println!("GET_BOOT_TIME=FAILED: {e}");
        }

        println!("OTA_RESULT=SUCCESS");
        println!("OTA_VERSION={}", self.target_version);
        println!("OTA_SLOT_FINAL={}", current_slot);
        println!("OTA_WIPE_OVERLAYS_FINAL={}", wipe_overlays_status);

        info!("OTA update completed successfully!");
        Ok(())
    }

    async fn connect(&self) -> Result<SshWrapper> {
        info!(
            "Connecting to Orb device at {}:{}",
            self.hostname, self.port
        );

        let auth = match (&self.password, &self.key_path) {
            (Some(password), None) => AuthMethod::Password(password.clone()),
            (None, Some(key_path)) => AuthMethod::Key {
                private_key_path: key_path.clone(),
            },
            _ => unreachable!("Clap ensures exactly one auth method is specified"),
        };

        let connect_args = SshConnectArgs {
            hostname: self.hostname.clone(),
            port: self.port,
            username: self.username.clone(),
            auth,
        };

        let session = SshWrapper::connect(connect_args)
            .await
            .wrap_err("Failed to establish SSH connection to Orb device")?;

        info!("Successfully connected to Orb device");
        Ok(session)
    }

    #[instrument(skip_all)]
    async fn get_current_slot(&self, session: &SshWrapper) -> Result<String> {
        info!("Determining current slot");
        let result = session
            .execute_command("TERM=dumb orb-slot-ctrl -c")
            .await
            .wrap_err("Failed to execute orb-slot-ctrl -c")?;

        ensure!(
            result.is_success(),
            "orb-slot-ctrl -c failed: {}",
            result.stderr
        );

        let slot_letter = if result.stdout.contains('a') {
            'a'
        } else if result.stdout.contains('b') {
            'b'
        } else {
            bail!("Could not parse current slot from: {}", result.stdout);
        };

        let slot_name = format!("slot_{slot_letter}");
        info!("Current slot: {}", slot_name);
        Ok(slot_name)
    }

    #[instrument(skip_all)]
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

        ensure!(
            result.is_success(),
            "Failed to read versions.json: {}",
            result.stderr
        );

        let mut versions_data: Value = serde_json::from_str(&result.stdout)
            .wrap_err("Failed to parse versions.json")?;

        let version_with_prefix = format!("to-{}", self.target_version);
        let releases = versions_data
            .get_mut("releases")
            .ok_or_else(|| eyre!("releases field not found in versions.json"))?;

        let releases_obj = releases
            .as_object_mut()
            .ok_or_else(|| eyre!("releases field is not an object in versions.json"))?;

        releases_obj.insert(
            current_slot.to_string(),
            Value::String(version_with_prefix.clone()),
        );

        info!(
            "Updated {} to version: {}",
            current_slot, version_with_prefix
        );

        let updated_json_str = serde_json::to_string_pretty(&versions_data)
            .wrap_err("Failed to serialize updated versions.json")?;

        let result = session
            .execute_command(&format!(
                "echo '{updated_json_str}' | sudo tee /usr/persistent/versions.json > /dev/null"
            ))
            .await
            .wrap_err("Failed to write updated versions.json")?;

        ensure!(
            result.is_success(),
            "Failed to write versions.json: {}",
            result.stderr
        );

        info!("versions.json updated successfully");
        Ok(())
    }

    async fn wipe_overlays(&self, session: &SshWrapper) -> Result<()> {
        let result = session
            .execute_command("bash -c 'source ~/.bash_profile 2>/dev/null || true; source ~/.bashrc 2>/dev/null || true; wipe_overlays'")
            .await
            .wrap_err("Failed to execute wipe_overlays function")?;

        ensure!(
            result.is_success(),
            "wipe_overlays function failed: {}",
            result.stderr
        );

        info!("Overlays wiped successfully");
        Ok(())
    }

    #[instrument(skip_all)]
    async fn restart_update_agent(&self, session: &SshWrapper) -> Result<()> {
        info!("Restarting worldcoin-update-agent.service");

        let result = session
            .execute_command(
                "TERM=dumb sudo systemctl restart worldcoin-update-agent.service",
            )
            .await
            .wrap_err("Failed to restart worldcoin-update-agent.service")?;

        ensure!(
            result.is_success(),
            "Failed to restart worldcoin-update-agent.service: {}",
            result.stderr
        );

        info!("worldcoin-update-agent.service restarted successfully");
        Ok(())
    }

    #[instrument(skip_all)]
    async fn monitor_update_progress(&self, session: &SshWrapper) -> Result<()> {
        const MAX_WAIT_SECONDS: u64 = 7200;
        const POLL_INTERVAL: u64 = 3;

        info!("Starting  monitoring of update progress");
        let start_time = Instant::now();
        let timeout = Duration::from_secs(MAX_WAIT_SECONDS);
        let mut seen_lines = std::collections::HashSet::new();
        let mut consecutive_failures = 0;
        const MAX_CONSECUTIVE_FAILURES: u32 = 10;

        while start_time.elapsed() < timeout {
            match self.fetch_new_log_lines(session, &mut seen_lines).await {
                Ok(new_lines) => {
                    consecutive_failures = 0;
                    for line in new_lines {
                        println!("{}", line.trim());

                        if line.contains("waiting 10 seconds before reboot to allow propagation to backend") {
                            info!("Reboot message detected: {}", line.trim());
                            return Ok(());
                        }
                        if line.contains("worldcoin-update-agent.service: Main process exited, code=exited, status=1/FAILURE") {
                            error!("Update agent service failed: {}", line.trim());
                            bail!("Update agent service failed - update installation failed");
                        }

                        if line.contains("ERROR")
                            || line.contains("FATAL")
                            || line.contains("CRITICAL")
                        {
                            warn!(
                                "Critical error detected in update logs: {}",
                                line.trim()
                            );
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
                        error!("Too many consecutive failures ({}) fetching logs, update may have failed", consecutive_failures);
                        bail!("Too many consecutive failures fetching update logs");
                    }
                }
            }

            tokio::time::sleep(Duration::from_secs(POLL_INTERVAL)).await;
        }

        error!(
            "Timeout waiting for update completion within {} seconds",
            MAX_WAIT_SECONDS
        );
        bail!(
            "Timeout waiting for update completion within {} seconds",
            MAX_WAIT_SECONDS
        );
    }

    #[instrument(skip_all)]
    async fn fetch_new_log_lines(
        &self,
        session: &SshWrapper,
        seen_lines: &mut std::collections::HashSet<String>,
    ) -> Result<Vec<String>> {
        let command = "TERM=dumb sudo journalctl -u worldcoin-update-agent.service --no-pager -n 100";

        let result = session
            .execute_command(command)
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

    #[instrument(skip_all)]
    async fn handle_reboot(&self, log_suffix: &str) -> Result<SshWrapper> {
        info!("Waiting for reboot and device to come back online");

        self.capture_boot_logs_during_reboot(log_suffix).await?;

        let start_time = Instant::now();
        let timeout = Duration::from_secs(900); // 15 minutes
        let mut attempt_count = 0;
        const MAX_ATTEMPTS: u32 = 90;
        let mut last_error = None;

        while start_time.elapsed() < timeout && attempt_count < MAX_ATTEMPTS {
            attempt_count += 1;
            tokio::time::sleep(Duration::from_secs(10)).await;

            debug!(
                "Attempting to reconnect (attempt {}/{})",
                attempt_count, MAX_ATTEMPTS
            );

            match self.connect().await {
                Ok(session) => match session.test_connection().await {
                    Ok(_) => {
                        info!("Device is back online and responsive after reboot (attempt {})", attempt_count);
                        return Ok(session);
                    }
                    Err(e) => {
                        debug!(
                            "Connection test failed on attempt {}: {}",
                            attempt_count, e
                        );
                        last_error = Some(e);
                    }
                },
                Err(e) => {
                    debug!(
                        "Device not yet available on attempt {}: {}",
                        attempt_count, e
                    );
                    last_error = Some(e);
                }
            }
        }

        let elapsed = start_time.elapsed();
        error!(
            "Device did not come back online within {:?} (attempted {} times)",
            elapsed, attempt_count
        );

        let error_context = if let Some(ref err) = last_error {
            format!("Last error: {err}")
        } else {
            "No specific error captured".to_string()
        };

        bail!(
            "Device did not come back online within {:?} (attempted {} times). {}",
            elapsed,
            attempt_count,
            error_context
        );
    }

    #[instrument(skip_all)]
    async fn capture_boot_logs_during_reboot(&self, log_suffix: &str) -> Result<()> {
        info!("Starting boot log capture for {}", log_suffix);

        let boot_log_path = self
            .log_file
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join(format!("boot_log_{log_suffix}.txt"));

        let serial_path = self.get_serial_path()?;
        let serial = tokio_serial::new(
            &*serial_path.to_string_lossy(),
            crate::serial::ORB_BAUD_RATE,
        )
        .open_native_async()
        .wrap_err_with(|| {
            format!("Failed to open serial port {}", serial_path.display())
        })?;

        let (serial_reader, _serial_writer) = tokio::io::split(serial);
        let (serial_output_tx, serial_output_rx) = broadcast::channel(64);
        let (reader_task, kill_tx) =
            spawn_serial_reader_task(serial_reader, serial_output_tx);

        let boot_log_fut = async {
            let mut boot_log_content = Vec::new();
            let mut serial_stream = BroadcastStream::new(serial_output_rx);
            let timeout = Duration::from_secs(300);

            let start_time = Instant::now();
            while start_time.elapsed() < timeout {
                match tokio::time::timeout(Duration::from_secs(1), serial_stream.next())
                    .await
                {
                    Ok(Some(Ok(bytes))) => {
                        boot_log_content.extend_from_slice(&bytes);

                        if let Ok(text) = String::from_utf8(bytes.to_vec()) {
                            if text.contains(LOGIN_PROMPT_PATTERN) {
                                info!("Login prompt detected in boot logs, stopping capture");
                                break;
                            }
                        }
                    }
                    Ok(Some(Err(e))) => {
                        warn!("Error reading serial stream: {}", e);
                    }
                    Ok(None) => {
                        warn!("Serial stream ended unexpectedly");
                        break;
                    }
                    Err(_) => {
                        continue;
                    }
                }
            }

            if !boot_log_content.is_empty() {
                tokio::fs::write(&boot_log_path, &boot_log_content)
                    .await
                    .wrap_err_with(|| {
                        format!(
                            "Failed to write boot log to {}",
                            boot_log_path.display()
                        )
                    })?;

                info!("Boot log saved to: {}", boot_log_path.display());
            } else {
                warn!("No boot log content captured");
            }

            let _ = kill_tx.send(());
            Ok::<(), color_eyre::Report>(())
        };

        tokio::try_join! {
            boot_log_fut,
            async {
                reader_task.await.wrap_err("serial reader task panicked")?
            },
        }?;

        Ok(())
    }

    #[instrument(skip_all)]
    async fn run_update_verifier(&self, session: &SshWrapper) -> Result<()> {
        info!("Running orb-update-verifier");

        let result = session
            .execute_command("TERM=dumb sudo orb-update-verifier")
            .await
            .wrap_err("Failed to run orb-update-verifier")?;

        ensure!(
            result.is_success(),
            "orb-update-verifier failed: {}",
            result.stderr
        );

        info!("orb-update-verifier succeeded: {}", result.stdout);
        Ok(())
    }

    #[instrument(skip_all)]
    async fn get_capsule_update_status(&self, session: &SshWrapper) -> Result<()> {
        info!("Getting capsule update status");

        let result = session
            .execute_command("TERM=dumb sudo nvbootctrl dump-slots-info")
            .await
            .wrap_err("Failed to get capsule update status")?;

        // Note: nvbootctrl returns exit code 1 with "Error: can not open /dev/mem" but still outputs valid info
        // So we don't check is_success() here, just parse the output

        let capsule_status = result
            .stdout
            .lines()
            .find(|line| line.starts_with("Capsule update status:"))
            .and_then(|line| line.split(':').nth(1).map(|s| s.trim().to_string()))
            .ok_or_else(|| {
                eyre!("Could not find 'Capsule update status' in nvbootctrl output")
            })?;

        println!("CAPSULE_UPDATE_STATUS={}", capsule_status);

        info!("Capsule update status: {}", capsule_status);
        Ok(())
    }

    #[instrument(skip_all)]
    async fn run_check_my_orb(&self, session: &SshWrapper) -> Result<()> {
        info!("Running check-my-orb");

        let result = session
            .execute_command("TERM=dumb check-my-orb")
            .await
            .wrap_err("Failed to run check-my-orb")?;

        if !result.is_success() {
            warn!("check-my-orb failed with exit code: {}", result.stderr);
            println!("CHECK_MY_ORB_STATUS=FAILED");
        } else {
            println!("CHECK_MY_ORB_STATUS=SUCCESS");
            info!("check-my-orb completed successfully");
        }

        println!("CHECK_MY_ORB_OUTPUT_START");
        println!("{}", result.stdout);
        println!("CHECK_MY_ORB_OUTPUT_END");

        Ok(())
    }

    #[instrument(skip_all)]
    async fn get_boot_time(&self, session: &SshWrapper) -> Result<()> {
        info!("Getting last boot time");

        let result = session
            .execute_command("TERM=dumb systemd-analyze time")
            .await
            .wrap_err("Failed to run systemd-analyze")?;

        ensure!(
            result.is_success(),
            "systemd-analyze failed: {}",
            result.stderr
        );

        println!("BOOT_TIME");
        println!("{}", result.stdout);
        Ok(())
    }
}
