use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::mcu_util::{
    check_jetson_post_ota, check_main_board_versions_match,
    check_security_board_versions_match,
};
use crate::{AuthMethod, RemoteConnectArgs, RemoteSession, RemoteTransport};
use clap::Parser;
use color_eyre::{
    eyre::{bail, ContextCompat, WrapErr},
    Result,
};
use secrecy::SecretString;
use tracing::{error, info, instrument};

use crate::orb::{OrbConfig, Platform};

mod monitor;
mod reboot;
mod system;

use crate::verify;

#[derive(Debug, Parser)]
#[command(group = clap::ArgGroup::new("auth").multiple(false))]
pub struct Ota {
    /// Target version to update to
    #[arg(long)]
    target_version: String,

    /// Transport used to connect to the Orb device
    #[arg(long, value_enum, default_value_t = RemoteTransport::Ssh)]
    transport: RemoteTransport,

    #[command(flatten)]
    orb_config: OrbConfig,

    /// Username
    #[arg(long)]
    username: Option<String>,

    /// Password for authentication (mutually exclusive with --key-path)
    #[arg(long, group = "auth")]
    password: Option<SecretString>,

    /// Path to SSH private key for authentication (mutually exclusive with --password)
    #[arg(long, group = "auth")]
    key_path: Option<PathBuf>,

    /// SSH port for the Orb device
    #[arg(long, default_value = "22")]
    port: u16,

    /// Timeout for the entire OTA process in seconds
    #[arg(long, default_value = "7200")] // 2 hours by default
    timeout_secs: u64,

    /// Path to save journalctl logs from worldcoin-update-agent.service
    #[arg(long)]
    log_file: PathBuf,
}

impl Ota {
    /// Get the serial port path from orb_config
    fn get_serial_path(orb_config: &OrbConfig) -> Result<&PathBuf> {
        orb_config
            .serial_path
            .as_ref()
            .wrap_err("serial-path must be specified")
    }

    #[instrument]
    pub async fn run(self) -> Result<()> {
        let _start_time = Instant::now();
        info!("Starting OTA update to version: {}", self.target_version);

        let orb_config = self.orb_config.use_file_if_exists()?;

        let session = self.connect_remote(&orb_config).await.inspect_err(|e| {
            println!("OTA_RESULT=FAILED");
            println!("OTA_ERROR=REMOTE_CONNECTION_FAILED: {e}");
        })?;

        let platform = orb_config
            .platform
            .wrap_err("platform must be specified for OTA")?;

        let (session, wipe_overlays_status) = match platform {
            Platform::Diamond | Platform::Pearl => {
                info!("Wiping overlays before update");
                system::wipe_overlays(&session).await.inspect_err(|e| {
                    error!("Failed to wipe overlays: {}", e);
                })?;
                info!("Overlays wiped successfully, rebooting device");

                system::reboot_orb(&session).await?;
                info!("Reboot command sent to Orb device");

                let new_session = self
                    .handle_reboot("wipe_overlays", &orb_config)
                    .await
                    .inspect_err(|e| {
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
        let session = self
            .handle_reboot("update", &orb_config)
            .await
            .inspect_err(|e| {
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
        let platform_name = self
            .orb_config
            .platform
            .map(|p| format!("{}", p))
            .unwrap_or_else(|| "unknown".to_string());
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

    async fn connect_remote(&self, orb_config: &OrbConfig) -> Result<RemoteSession> {
        const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
        let auth = self.resolve_remote_auth()?;

        let hostname = orb_config
            .get_hostname()
            .wrap_err("orb-id must be specified to derive hostname")?;

        info!("Connecting to Orb device at {}:{}", hostname, self.port);

        let connect_args = RemoteConnectArgs {
            transport: self.transport,
            hostname: Some(hostname),
            orb_id: orb_config.orb_id.clone(),
            username: self.username.clone(),
            port: self.port,
            auth,
            timeout: CONNECT_TIMEOUT,
        };

        let session = RemoteSession::connect(connect_args)
            .await
            .wrap_err("Failed to establish remote connection to Orb device")?;

        info!("Successfully connected to Orb device");

        Ok(session)
    }

    fn resolve_remote_auth(&self) -> Result<Option<AuthMethod>> {
        match self.transport {
            RemoteTransport::Ssh => match (&self.password, &self.key_path) {
                (Some(password), None) => {
                    Ok(Some(AuthMethod::Password(password.clone())))
                }
                (None, Some(private_key_path)) => Ok(Some(AuthMethod::Key {
                    private_key_path: private_key_path.clone(),
                })),
                (None, None) => {
                    bail!("--transport ssh requires --password or --key-path")
                }
                (Some(_), Some(_)) => {
                    bail!("--password and --key-path are mutually exclusive")
                }
            },
            RemoteTransport::Teleport => {
                if self.password.is_some() || self.key_path.is_some() {
                    bail!(
                        "--password/--key-path can only be used with --transport ssh"
                    );
                }
                if self.port != 22 {
                    bail!("--transport teleport does not use --port (must be 22)");
                }

                Ok(None)
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn sample_ota() -> Ota {
        Ota {
            target_version: "test-version".to_owned(),
            transport: RemoteTransport::Ssh,
            orb_config: OrbConfig::builder()
                .orb_id("test-host".to_owned())
                .platform(Platform::Diamond)
                .serial_path(PathBuf::from("/dev/null"))
                .pin_ctrl_type(crate::orb::PinControlType::Ftdi)
                .build(),
            username: None,
            password: None,
            key_path: None,
            port: 22,
            timeout_secs: 7200,
            log_file: PathBuf::from("/tmp/ota.log"),
            serial_path: Some(PathBuf::from("/dev/null")),
            serial_id: None,
            pin_ctrl: PinCtrl {
                pin_ctrl_type: "ftdi".to_string(),
                ftdi_serial_number: None,
                ftdi_description: None,
                relay_power_bank: 0,
                relay_recovery_channel: 1,
                relay_power_channel: 2,
            },
        }
    }

    #[test]
    fn ssh_transport_requires_auth() {
        let ota = sample_ota();
        let err = ota
            .resolve_remote_auth()
            .expect_err("ssh must require auth");
        assert!(err
            .to_string()
            .contains("--transport ssh requires --password or --key-path"));
    }

    #[test]
    fn ssh_transport_accepts_password_auth() {
        let mut ota = sample_ota();
        ota.password = Some(SecretString::from("password".to_owned()));

        let auth = ota
            .resolve_remote_auth()
            .expect("password auth should be accepted");
        assert!(matches!(auth, Some(AuthMethod::Password(_))));
    }

    #[test]
    fn teleport_transport_rejects_auth_flags() {
        let mut ota = sample_ota();
        ota.transport = RemoteTransport::Teleport;
        ota.password = Some(SecretString::from("password".to_owned()));

        let err = ota
            .resolve_remote_auth()
            .expect_err("teleport must reject ssh auth flags");
        assert!(err
            .to_string()
            .contains("--password/--key-path can only be used with --transport ssh"));
    }

    #[test]
    fn teleport_transport_rejects_custom_port() {
        let mut ota = sample_ota();
        ota.transport = RemoteTransport::Teleport;
        ota.port = 3022;

        let err = ota
            .resolve_remote_auth()
            .expect_err("teleport must reject custom ssh port");
        assert!(err
            .to_string()
            .contains("--transport teleport does not use --port"));
    }
}
