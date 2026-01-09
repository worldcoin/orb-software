use crate::commands::SetRecoveryPin;
use crate::ftdi::OutputState;
use crate::serial::{spawn_serial_reader_task, LOGIN_PROMPT_PATTERN};
use color_eyre::{
    eyre::{bail, WrapErr},
    Result,
};
use futures::StreamExt;
use orb_hil::SshWrapper;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use tokio_serial::SerialPortBuilderExt;
use tokio_stream::wrappers::BroadcastStream;
use tracing::{debug, error, info, instrument, warn};

use super::Ota;

impl Ota {
    #[instrument(skip_all)]
    pub(super) async fn handle_reboot(&self, log_suffix: &str) -> Result<SshWrapper> {
        info!("Waiting for reboot and device to come back online");

        // Always wait for SSH to become unreachable before holding the recovery pin.
        // This ensures we time it with the actual shutdown, not based on assumptions.
        // - For manual reboots (wipe_overlays): reboot command was just sent, wait for SSH to die
        // - For update-initiated reboots: update-agent will reboot, wait for SSH to die
        info!("Monitoring SSH connection to detect when shutdown actually begins");
        self.wait_for_ssh_disconnection(Duration::from_secs(30))
            .await?;
        info!("SSH disconnected - system is shutting down, holding recovery pin");

        // Hold for 20s to cover systemd shutdown + power cycle + early boot
        let hold_duration = 20;

        // Set recovery pin HIGH to prevent entering recovery mode
        info!(
            "Setting recovery pin HIGH to prevent recovery mode during reboot (hold duration: {}s)",
            hold_duration
        );
        let set_recovery = SetRecoveryPin {
            state: OutputState::High,
            serial_num: None,
            desc: None,
            duration: hold_duration,
        };

        // Run recovery pin setting in background task
        let recovery_task = tokio::spawn(async move {
            set_recovery
                .run()
                .await
                .wrap_err("failed to set recovery pin")
        });

        // Wait for recovery pin task to complete
        recovery_task
            .await
            .wrap_err("recovery pin task panicked")??;

        // Brief delay to allow USB device to be re-enumerated and udev rules to apply
        // after FTDI GPIO is released. The FTDI device detaches/reattaches kernel
        // drivers which causes /dev/ttyUSB* to be recreated.
        tokio::time::sleep(Duration::from_millis(200)).await;

        self.capture_boot_logs(log_suffix).await?;

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

            match self.connect_ssh().await {
                Ok(session) => match session.test_connection().await {
                    Ok(_) => {
                        info!("Device is back online and responsive after reboot (attempt {})", attempt_count);

                        info!("Waiting for NTP time synchronization after reboot");
                        match super::system::wait_for_time_sync(&session).await {
                            Ok(_) => {
                                info!("NTP time synchronized successfully");
                                return Ok(session);
                            }
                            Err(e) => {
                                debug!(
                                    "Time sync failed on attempt {}: {}",
                                    attempt_count, e
                                );
                                last_error = Some(e);
                            }
                        }
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
    async fn capture_boot_logs(&self, log_suffix: &str) -> Result<()> {
        let platform_name = format!("{:?}", self.platform).to_lowercase();
        info!(
            "Starting boot log capture for {} ({})",
            log_suffix, platform_name
        );

        let boot_log_path = self
            .log_file
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join(format!("boot_log_{platform_name}_{log_suffix}.txt"));

        // Create parent directory if it doesn't exist
        if let Some(parent) = boot_log_path.parent()
            && let Err(e) = tokio::fs::create_dir_all(parent).await
        {
            warn!(
                "Failed to create directory {}: {}. Boot log capture may fail.",
                parent.display(),
                e
            );
        }

        let serial_path = match self.get_serial_path() {
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
            use tokio::io::AsyncWriteExt;

            // Open file for writing incrementally
            let mut log_file = match tokio::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&boot_log_path)
                .await
            {
                Ok(f) => Some(f),
                Err(e) => {
                    warn!(
                        "Failed to open boot log file {}: {}. Will continue without writing to disk.",
                        boot_log_path.display(),
                        e
                    );
                    None
                }
            };

            let mut total_bytes = 0;
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
                        // Write to file immediately as data arrives
                        if let Some(ref mut file) = log_file {
                            if let Err(e) = file.write_all(&bytes).await {
                                warn!("Failed to write to boot log file: {}. Continuing capture in memory only.", e);
                                log_file = None;
                            } else {
                                // Flush to ensure data is written to disk immediately
                                let _ = file.flush().await;
                                total_bytes += bytes.len();
                            }
                        }

                        if let Ok(text) = String::from_utf8(bytes.to_vec())
                            && text.contains(LOGIN_PROMPT_PATTERN)
                        {
                            info!("Login prompt detected in boot logs after {:?}, stopping capture", start_time.elapsed());
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
                    "Boot log capture timed out after {:?} without finding login prompt. Will proceed with SSH reconnection anyway.",
                    timeout
                );
            }

            // Final flush and close
            if let Some(mut file) = log_file {
                let _ = file.flush().await;
                let _ = file.shutdown().await;
                info!(
                    "Boot log saved to: {} ({} bytes)",
                    boot_log_path.display(),
                    total_bytes
                );
            } else if total_bytes == 0 {
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

    /// Wait for SSH connection to become unreachable, indicating shutdown has started
    #[instrument(skip_all)]
    async fn wait_for_ssh_disconnection(&self, timeout: Duration) -> Result<()> {
        let start = Instant::now();
        let mut attempt = 0;

        loop {
            if start.elapsed() > timeout {
                bail!("SSH did not disconnect within {:?}", timeout);
            }

            attempt += 1;

            // Try to establish connection with a lightweight command
            match self.connect_ssh().await {
                Ok(session) => match session.execute_command("echo").await {
                    Ok(_) => {
                        // SSH still alive, system hasn't started shutting down yet
                        debug!(
                            "SSH still responsive (attempt {}), waiting for shutdown...",
                            attempt
                        );
                        tokio::time::sleep(Duration::from_millis(500)).await;
                    }
                    Err(_) => {
                        // Command failed but connection succeeded - might be shutting down
                        info!("SSH connection degraded, shutdown likely in progress");
                        return Ok(());
                    }
                },
                Err(_) => {
                    // Can't connect - shutdown has started
                    info!(
                        "SSH connection lost after {} attempts, shutdown confirmed",
                        attempt
                    );
                    return Ok(());
                }
            }
        }
    }
}
