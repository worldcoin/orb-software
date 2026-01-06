use std::path::PathBuf;
use std::time::Instant;

use clap::Parser;
use color_eyre::{
    eyre::{bail, WrapErr},
    Result,
};
use orb_hil::mcu_util::{
    check_jetson_post_ota, check_main_board_versions_match,
    check_security_board_versions_match,
};
use orb_hil::{AuthMethod, SshConnectArgs, SshWrapper};
use secrecy::SecretString;
use tracing::{error, info, instrument};

mod monitor;
mod reboot;
mod system;

use orb_hil::verify;

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
    password: Option<SecretString>,

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

        // Create log directory if it doesn't exist
        if let Some(log_dir) = self.log_file.parent() {
            tokio::fs::create_dir_all(log_dir).await.wrap_err_with(|| {
                format!("Failed to create log directory: {}", log_dir.display())
            })?;
            info!("Log directory created/verified: {}", log_dir.display());
        }

        let session = self.connect_ssh().await.inspect_err(|e| {
            println!("OTA_RESULT=FAILED");
            println!("OTA_ERROR=SSH_CONNECTION_FAILED: {e}");
        })?;

        let (session, wipe_overlays_status) = match self.platform {
            Platform::Diamond | Platform::Pearl => {
                info!("Wiping overlays before update");
                system::wipe_overlays(&session).await.inspect_err(|e| {
                    error!("Failed to wipe overlays: {}", e);
                })?;
                info!("Overlays wiped successfully");

                info!("Waiting for NTP time synchronization before reboot");
                system::wait_for_time_sync(&session)
                    .await
                    .inspect_err(|e| {
                        error!("Failed to sync time before reboot: {}", e);
                    })?;
                info!("NTP time synchronized, rebooting device");

                system::reboot_orb(&session).await?;
                info!("Reboot command sent to Orb device");

                let new_session =
                    self.handle_reboot("wipe_overlays").await.inspect_err(|e| {
                        error!(
                            "Failed to reboot and reconnect after wiping overlays: {}",
                            e
                        );
                    })?;
                (new_session, "succeeded".to_string())
            }
        };

        let current_slot =
            system::get_current_slot(&session).await.inspect_err(|e| {
                println!("OTA_RESULT=FAILED");
                println!("OTA_ERROR=SLOT_DETECTION_FAILED: {e}");
            })?;
        info!("Current slot detected: {}", current_slot);

        println!("OTA_SLOT={}", current_slot);
        println!("OTA_WIPE_OVERLAYS={}", wipe_overlays_status);

        info!(
            "Updating /usr/persistent/versions.json for slot {}",
            current_slot
        );
        system::update_versions_json(&session, &current_slot, &self.target_version)
            .await
            .inspect_err(|e| {
                println!("OTA_RESULT=FAILED");
                println!("OTA_ERROR=VERSION_UPDATE_FAILED: {e}");
            })?;
        info!("versions.json updated successfully");

        info!("Waiting for system time synchronization");
        system::wait_for_time_sync(&session)
            .await
            .inspect_err(|e| {
                println!("OTA_RESULT=FAILED");
                println!("OTA_ERROR=TIME_SYNC_FAILED: {e}");
            })?;
        info!("System time synchronized");

        info!("Restarting worldcoin-update-agent.service");
        let start_timestamp = system::restart_update_agent(&session)
            .await
            .inspect_err(|e| {
                println!("OTA_RESULT=FAILED");
                println!("OTA_ERROR=UPDATE_AGENT_RESTART_FAILED: {e}");
            })?;
        info!("worldcoin-update-agent.service restarted successfully, start timestamp: {}", start_timestamp);

        info!("Starting update progress and service status monitoring");
        let _log_lines = monitor::monitor_update_progress(&session, &start_timestamp)
            .await
            .inspect_err(|e| {
                println!("OTA_RESULT=FAILED");
                println!("OTA_ERROR=UPDATE_PROGRESS_FAILED: {e}");
            })?;
        // Note: log lines are printed in real-time during monitoring

        // After successful update update-agent reboots the orb
        let session = self.handle_reboot("update").await.inspect_err(|e| {
            println!("OTA_RESULT=FAILED");
            println!("OTA_ERROR=POST_UPDATE_REBOOT_FAILED: {e}");
        })?;
        info!("Device successfully rebooted and reconnected - update application completed");

        info!("Running orb-update-verifier");
        let verifier_output =
            verify::run_update_verifier(&session)
                .await
                .inspect_err(|e| {
                    println!("OTA_RESULT=FAILED");
                    println!("OTA_ERROR=UPDATE_VERIFIER_FAILED: {e}");
                })?;
        info!("orb-update-verifier succeeded: {}", verifier_output);

        info!("Getting capsule update status");
        let capsule_status = verify::get_capsule_update_status(&session)
            .await
            .inspect_err(|e| {
                println!("OTA_RESULT=FAILED");
                println!("OTA_ERROR=CAPSULE_UPDATE_STATUS_FAILED: {e}");
            })?;
        println!("CAPSULE_UPDATE_STATUS={}", capsule_status);
        info!("Capsule update status: {}", capsule_status);

        info!("Running check-my-orb");
        match verify::run_check_my_orb(&session).await {
            Ok(output) => {
                println!("CHECK_MY_ORB_STATUS=SUCCESS");
                info!("check-my-orb completed successfully");
                println!("CHECK_MY_ORB_OUTPUT_START");
                println!("{output}");
                println!("CHECK_MY_ORB_OUTPUT_END");
            }
            Err(e) => {
                println!("CHECK_MY_ORB_EXECUTION_FAILED: {e}");
                println!("CHECK_MY_ORB_STATUS=FAILED");
            }
        }

        info!("Getting hardware states");
        match verify::run_mcu_util_info(&session).await {
            Ok(output) => {
                match check_main_board_versions_match(&output) {
                    Ok(true) => {
                        if let Ok(true) = check_jetson_post_ota(&output) {
                            println!("MAIN_MCU_POST_OTA_STATUS=SUCCESS");
                        } else {
                            println!("MAIN_MCU_POST_OTA_STATUS=FAILED");
                        }
                    }
                    Ok(false) => {
                        println!("MAIN_MCU_POST_OTA_STATUS=FAILED");
                    }
                    Err(e) => {
                        println!("MAIN_MCU_POST_OTA_EXECUTION_FAILED: {e}");
                        println!("MAIN_MCU_POST_OTA_STATUS=FAILED");
                    }
                }
                match check_security_board_versions_match(&output) {
                    Ok(true) => {
                        println!("SECURITY_MCU_POST_OTA_STATUS=SUCCESS");
                    }
                    Ok(false) => {
                        println!("SECURITY_MCU_POST_OTA_STATUS=FAILED");
                    }
                    Err(e) => {
                        println!("SECURITY_MCU_POST_OTA_EXECUTION_FAILED: {e}");
                        println!("SECURITY_MCU_POST_OTA_STATUS=FAILED");
                    }
                }

                // print full output for easier debugging
                println!("ORB_MCU_UTIL_INFO_OUTPUT_START");
                println!("{output}");
                println!("ORB_MCU_UTIL_INFO_OUTPUT_END");
            }
            Err(e) => {
                println!("ORB_MCU_UTIL_INFO_EXECUTION_FAILED: {e}");
                println!("MCU_UTIL_STATUS=FAILED");
            }
        }

        info!("Getting last boot time");
        match verify::get_boot_time(&session).await {
            Ok(boot_time) => {
                println!("BOOT_TIME");
                println!("{boot_time}");
            }
            Err(e) => {
                println!("GET_BOOT_TIME=FAILED: {e}");
            }
        }

        println!("OTA_RESULT=SUCCESS");
        println!("OTA_VERSION={}", self.target_version);
        println!("OTA_SLOT_FINAL={}", current_slot);
        println!("OTA_WIPE_OVERLAYS_FINAL={}", wipe_overlays_status);

        // Print all result files for easy collection/upload
        self.print_result_files();

        info!("OTA update completed successfully!");
        Ok(())
    }

    fn print_result_files(&self) {
        let platform_name = format!("{:?}", self.platform).to_lowercase();
        let log_dir = self
            .log_file
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."));

        println!("\n========================================");
        println!("OTA TEST RESULT FILES");
        println!("========================================");

        let result_files = vec![
            self.log_file.clone(),
            log_dir.join(format!("boot_log_{}_wipe_overlays.txt", platform_name)),
            log_dir.join(format!("boot_log_{}_update.txt", platform_name)),
        ];

        println!("The following files contain OTA test results:");
        for file in &result_files {
            if file.exists() {
                println!("  ✓ {}", file.display());
            } else {
                println!("  ✗ {} (not found)", file.display());
            }
        }

        println!("\nTo upload all files:");
        println!("  # List of files:");
        for file in &result_files {
            if file.exists() {
                println!("  {}", file.display());
            }
        }
        println!("========================================\n");
    }

    async fn connect_ssh(&self) -> Result<SshWrapper> {
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
}
