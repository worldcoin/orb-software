use crate::serial::{spawn_serial_reader_task, LOGIN_PROMPT_PATTERN};
use crate::{orb_manager_from_config, BootMode, OrbConfig};

use crate::remote_cmd::RemoteSession;
use color_eyre::{
    eyre::{bail, WrapErr},
    Result,
};
use futures::StreamExt;
use std::path::Path;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use tokio_serial::SerialPortBuilderExt;
use tokio_stream::wrappers::BroadcastStream;
use tracing::{debug, error, info, instrument, warn};

use super::Ota;

impl Ota {
    #[instrument(skip_all)]
    pub(super) async fn handle_reboot(
        &self,
        log_suffix: &str,
        orb_config: &OrbConfig,
    ) -> Result<RemoteSession> {
        info!("Waiting for reboot and device to come back online");

        // Hold the recovery pin in normal-boot state for the entire boot process.
        //
        // For FTDI: set_boot_mode(Normal) sets RTS HIGH and holds the handle open.
        // For relays: set_boot_mode(Normal) turns off both power and recovery channels.
        let orb_config_for_pin = orb_config.clone();
        let (pin_release_tx, pin_release_rx) = std::sync::mpsc::channel::<()>();
        let recovery_task = tokio::task::spawn_blocking(move || -> Result<()> {
            let mut orb_mgr = orb_manager_from_config(&orb_config_for_pin)
                .wrap_err("failed to create pin controller")?;
            orb_mgr.set_boot_mode(BootMode::Normal)?;
            info!("✓ Recovery pin set to normal boot mode, waiting for boot");
            // Block until signaled or sender is dropped (error path).
            let _ = pin_release_rx.recv();
            info!("Recovery pin released");

            Ok(())
        });

        if let Some(log_file) = &self.log_file {
            Self::capture_boot_logs(log_file, log_suffix, orb_config).await?;
        }

        let start_time = Instant::now();
        let timeout = Duration::from_secs(900); // 15 minutes
        let mut attempt_count = 0;
        const MAX_ATTEMPTS: u32 = 90;
        let mut last_error = None;
        let mut found_session = None;

        while start_time.elapsed() < timeout && attempt_count < MAX_ATTEMPTS {
            attempt_count += 1;
            tokio::time::sleep(Duration::from_secs(10)).await;

            debug!(
                "Attempting to reconnect (attempt {}/{})",
                attempt_count, MAX_ATTEMPTS
            );

            match self.connect_remote(orb_config).await {
                Ok(session) => match session.test_connection().await {
                    Ok(_) => {
                        info!("Device is back online and responsive after reboot (attempt {})", attempt_count);
                        found_session = Some(session);
                        break;
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

        // Release the recovery pin now that the device is back online.
        let _ = pin_release_tx.send(());
        recovery_task
            .await
            .wrap_err("recovery pin task panicked")??;

        if let Some(session) = found_session {
            return Ok(session);
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
    async fn capture_boot_logs(
        log_file: &Path,
        log_suffix: &str,
        orb_config: &OrbConfig,
    ) -> Result<()> {
        let platform_name = if let Some(platform) = orb_config.platform {
            format!("{}", platform)
        } else {
            "unknown".to_string()
        };
        info!(
            "Starting boot log capture for {} ({})",
            log_suffix, platform_name
        );

        let boot_log_path = log_file
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join(format!("boot_log_{platform_name}_{log_suffix}.txt"));

        let serial_path = match Ota::get_serial_path(orb_config) {
            Ok(path) => path,
            Err(e) => {
                warn!(
                    "Failed to get serial path: {}. Skipping boot log capture.",
                    e
                );
                return Ok(());
            }
        };

        let serial = match tokio_serial::new(
            &*serial_path.to_string_lossy(),
            crate::serial::ORB_BAUD_RATE,
        )
        .open_native_async()
        {
            Ok(s) => s,
            Err(e) => {
                warn!(
                    "Failed to open serial port {}: {}. Skipping boot log capture.",
                    serial_path.display(),
                    e
                );
                return Ok(());
            }
        };

        let (serial_reader, _serial_writer) = tokio::io::split(serial);
        let (serial_output_tx, serial_output_rx) = broadcast::channel(64);
        let (reader_task, kill_tx) =
            spawn_serial_reader_task(serial_reader, serial_output_tx);

        let boot_log_fut = async {
            let mut boot_log_content = Vec::new();
            let mut serial_stream = BroadcastStream::new(serial_output_rx);
            // 3-minute timeout for flaky serial connections
            let timeout = Duration::from_secs(180);

            let start_time = Instant::now();
            let mut found_login_prompt = false;

            while start_time.elapsed() < timeout {
                match tokio::time::timeout(Duration::from_secs(1), serial_stream.next())
                    .await
                {
                    Ok(Some(Ok(bytes))) => {
                        boot_log_content.extend_from_slice(&bytes);

                        if let Ok(text) = String::from_utf8(bytes.to_vec())
                            && text.contains(LOGIN_PROMPT_PATTERN)
                        {
                            info!(
                                "Login prompt detected in boot logs after {:?}, stopping capture",
                                start_time.elapsed()
                            );
                            found_login_prompt = true;
                            break;
                        }
                    }
                    Ok(Some(Err(e))) => {
                        warn!("Error reading serial stream: {}", e);
                    }
                    Ok(None) => {
                        warn!(
                            "Serial stream ended unexpectedly after {:?}",
                            start_time.elapsed()
                        );
                        break;
                    }
                    Err(_) => {
                        continue;
                    }
                }
            }

            if start_time.elapsed() >= timeout && !found_login_prompt {
                warn!(
                    "Boot log capture timed out after {:?} without finding login prompt. \
                     Will proceed with SSH reconnection anyway.",
                    timeout
                );
            }

            if !boot_log_content.is_empty() {
                match tokio::fs::write(&boot_log_path, &boot_log_content).await {
                    Ok(_) => {
                        info!(
                            "Boot log saved to: {} ({} bytes)",
                            boot_log_path.display(),
                            boot_log_content.len()
                        );
                    }
                    Err(e) => {
                        warn!(
                            "Failed to write boot log to {}: {}. Continuing anyway.",
                            boot_log_path.display(),
                            e
                        );
                    }
                }
            } else {
                warn!("No boot log content captured from serial");
            }

            let _ = kill_tx.send(());
            Ok::<(), color_eyre::Report>(())
        };

        // Don't fail if serial capture has issues
        match tokio::try_join! {
            boot_log_fut,
            async {
                reader_task.await.wrap_err("serial reader task panicked")?
            },
        } {
            Ok(_) => info!("Boot log capture completed successfully"),
            Err(e) => warn!("Boot log capture had issues: {}. Continuing anyway.", e),
        }

        Ok(())
    }
}
