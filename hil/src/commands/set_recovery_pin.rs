use clap::Parser;
use color_eyre::{eyre::WrapErr as _, Result};
use std::time::Duration;

use crate::boot::{BUTTON_PIN, RECOVERY_PIN};
use crate::ftdi::{FtdiGpio, FtdiId, OutputState};

/// Set the recovery pin to a specific state without triggering the button
///
/// This is useful for ensuring the recovery pin has a known state before
/// OS-initiated reboots, preventing the device from entering recovery mode
/// unintentionally.
#[derive(Debug, Parser)]
pub struct SetRecoveryPin {
    /// Set the recovery pin state (high = normal boot, low = recovery mode)
    #[arg(value_parser = parse_pin_state)]
    pub state: OutputState,
    /// The serial number of the FTDI device to use
    #[arg(long, conflicts_with = "desc")]
    pub serial_num: Option<String>,
    /// The description of the FTDI device to use
    #[arg(long, conflicts_with = "serial_num")]
    pub desc: Option<String>,
    /// Keep the FTDI connection open to hold the pin state indefinitely
    /// (use Ctrl+C to release)
    #[arg(long, conflicts_with = "duration")]
    pub hold: bool,
    /// Hold the pin state for a specific duration in seconds
    /// (e.g., --duration 5 holds for 5 seconds)
    #[arg(long)]
    pub duration: Option<u64>,
}

fn parse_pin_state(s: &str) -> Result<OutputState> {
    match s.to_lowercase().as_str() {
        "high" | "1" | "normal" => Ok(OutputState::High),
        "low" | "0" | "recovery" => Ok(OutputState::Low),
        _ => Err(color_eyre::eyre::eyre!(
            "invalid state '{}', use 'high' or 'low'",
            s
        )),
    }
}

impl SetRecoveryPin {
    pub async fn run(self) -> Result<()> {
        let device = match (self.serial_num, self.desc) {
            (Some(serial), None) => Some(FtdiId::SerialNumber(serial)),
            (None, Some(desc)) => Some(FtdiId::Description(desc)),
            (None, None) => None,
            (Some(_), Some(_)) => unreachable!(),
        };

        let state_name = match self.state {
            OutputState::High => "HIGH (normal boot mode)",
            OutputState::Low => "LOW (recovery mode)",
        };

        if self.hold || self.duration.is_some() {
            let hold_duration = self.duration.map(Duration::from_secs);

            if let Some(dur) = hold_duration {
                tracing::info!(
                    "Setting recovery pin to {} and holding for {} seconds...",
                    state_name,
                    dur.as_secs()
                );
            } else {
                tracing::info!(
                    "Setting recovery pin to {} and holding indefinitely...",
                    state_name
                );
                tracing::info!("Press Ctrl+C to release the pin");
            }

            // Create a channel to signal when to release
            let (shutdown_tx, shutdown_rx) = std::sync::mpsc::channel::<()>();

            // Spawn a blocking task that holds the FTDI connection
            let state = self.state;
            let hold_task = std::thread::spawn(move || -> Result<()> {
                let mut ftdi = Self::make_ftdi(device)?;

                // IMPORTANT: Set button pin HIGH first to prevent power down
                // When FTDI enters bitbang mode, all pins default to LOW
                ftdi.set_pin(BUTTON_PIN, OutputState::High)?;

                // Now set recovery pin to desired state
                ftdi.set_pin(RECOVERY_PIN, state)?;

                tracing::info!("✓ Pin state set and holding (FTDI connection open)");

                // Block until shutdown signal or timeout
                if let Some(duration) = hold_duration {
                    let _ = shutdown_rx.recv_timeout(duration);
                } else {
                    let _ = shutdown_rx.recv();
                }

                tracing::info!("FTDI connection closing, pin will float");
                Ok(())
            });

            // If holding indefinitely, wait for Ctrl+C
            // If duration specified, wait for either Ctrl+C or timeout
            if hold_duration.is_some() {
                // Wait for thread to finish (will timeout after duration)
                hold_task
                    .join()
                    .map_err(|_| color_eyre::eyre::eyre!("hold task panicked"))??;
                tracing::info!("Duration elapsed, recovery pin released");
            } else {
                // Wait for Ctrl+C
                tokio::signal::ctrl_c()
                    .await
                    .wrap_err("failed to wait for ctrl+c")?;

                tracing::info!("Ctrl+C received, releasing recovery pin...");

                // Signal shutdown (dropping sender will close channel)
                drop(shutdown_tx);

                // Wait for the thread to finish
                hold_task
                    .join()
                    .map_err(|_| color_eyre::eyre::eyre!("hold task panicked"))??;
            }
        } else {
            tracing::info!("Setting recovery pin to {}", state_name);

            tokio::task::spawn_blocking(move || -> Result<()> {
                let mut ftdi = Self::make_ftdi(device)?;

                // IMPORTANT: Set button pin HIGH first to prevent power down
                // When FTDI enters bitbang mode, all pins default to LOW
                ftdi.set_pin(BUTTON_PIN, OutputState::High)?;

                // Now set recovery pin to desired state
                ftdi.set_pin(RECOVERY_PIN, self.state)?;

                // Note: Pin will float after FTDI is destroyed
                tracing::warn!(
                    "Pin state set, but will float after command exits. \
                     Use --hold or --duration to maintain state, or add a hardware pull-up resistor."
                );

                Ok(())
            })
            .await
            .wrap_err("task panicked")??;
        }

        Ok(())
    }

    fn make_ftdi(device: Option<FtdiId>) -> Result<FtdiGpio> {
        let builder = FtdiGpio::builder();
        let builder = match &device {
            Some(FtdiId::Description(desc)) => builder.with_description(desc),
            Some(FtdiId::SerialNumber(serial)) => builder.with_serial_number(serial),
            None => builder.with_default_device(),
        };
        builder
            .and_then(|b| b.configure())
            .wrap_err("failed to create ftdi device")
    }
}
