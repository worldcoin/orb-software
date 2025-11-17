use std::path::PathBuf;
use std::time::Instant;

use crate::ssh_wrapper::{AuthMethod, SshConnectArgs, SshWrapper};
use clap::Parser;
use color_eyre::{
    eyre::{bail, WrapErr},
    Result,
};
use tracing::{error, info, instrument};

mod monitor;
mod reboot;
mod system;
mod verify;

#[derive(Parser)]
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

impl std::fmt::Debug for Ota {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Hide the password if there is one
        f.debug_struct("Ota")
            .field("target_version", &self.target_version)
            .field("hostname", &self.hostname)
            .field("username", &self.username)
            .field("password", &self.password.as_ref().map(|_| "********"))
            .field("key_path", &self.key_path)
            .field("port", &self.port)
            .field("platform", &self.platform)
            .field("timeout_secs", &self.timeout_secs)
            .field("log_file", &self.log_file)
            .field("serial_path", &self.serial_path)
            .field("serial_id", &self.serial_id)
            .finish()
    }
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

    /// Main orchestration method for OTA update process
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

                        self.reboot_orb(&session).await?;
                        info!("Reboot command sent to Orb device");

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

        let start_timestamp = match self.restart_update_agent(&session).await {
            Ok(timestamp) => timestamp,
            Err(e) => {
                println!("OTA_RESULT=FAILED");
                println!("OTA_ERROR=UPDATE_AGENT_RESTART_FAILED: {e}");
                return Err(e);
            }
        };

        info!("Starting update progress and service status monitoring");
        if let Err(e) = self
            .monitor_update_progress(&session, &start_timestamp)
            .await
        {
            println!("OTA_RESULT=FAILED");
            println!("OTA_ERROR=UPDATE_PROGRESS_FAILED: {e}");
            return Err(e);
        }

        // After succesful update update-agent reboots the orb
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
}
