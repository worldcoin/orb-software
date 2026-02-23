use clap::Parser;
use color_eyre::{eyre::WrapErr as _, Result};
use std::time::Duration;

use crate::ftdi::OutputState;
use crate::commands::PinCtrl;

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
    /// Hold the pin state for a specific duration in seconds
    /// (e.g., --duration 10 holds for 10 seconds, then exits)
    /// Default is 5 seconds
    #[arg(long, default_value = "5")]
    pub duration: u64,
    #[command(flatten)]
    pub pin_ctrl: PinCtrl,
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
            let mut controller = self
                .pin_ctrl
                .build_controller()
                .wrap_err("failed to create pin controller")?;

            // Set recovery pin to desired state
            // Recovery enabled when state is Low
            let recovery_enabled = matches!(state, OutputState::Low);
            controller.set_recovery(recovery_enabled)?;

            tracing::info!("✓ Pin state set and holding (controller connection open)");

            // Hold for specified duration
            std::thread::sleep(hold_duration);

            tracing::info!("Duration elapsed, controller connection closing");
            Ok(())
        })
        .await
        .wrap_err("task panicked")??;

        Ok(())
    }
}
