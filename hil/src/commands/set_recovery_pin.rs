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
    /// Hold the pin state for a specific duration in seconds
    /// (e.g., --duration 10 holds for 10 seconds, then exits)
    /// Default is 5 seconds
    #[arg(long, default_value = "5")]
    pub duration: u64,
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

        tracing::info!(
            "Setting recovery pin to {} and holding for {} seconds...",
            state_name,
            self.duration
        );

        let hold_duration = Duration::from_secs(self.duration);
        let state = self.state;

        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut ftdi = Self::make_ftdi(device)?;

            // IMPORTANT: Set button pin HIGH first to prevent power down
            // When FTDI enters bitbang mode, all pins default to LOW
            ftdi.set_pin(BUTTON_PIN, OutputState::High)?;

            // Now set recovery pin to desired state
            ftdi.set_pin(RECOVERY_PIN, state)?;

            tracing::info!("✓ Pin state set and holding (FTDI connection open)");

            // Hold for specified duration
            std::thread::sleep(hold_duration);

            tracing::info!("Duration elapsed, FTDI connection closing");
            Ok(())
        })
        .await
        .wrap_err("task panicked")??;

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
