use clap::Parser;
use color_eyre::{eyre::WrapErr as _, Result};
use std::time::Duration;
use tracing::info;

use crate::boot::BUTTON_PIN;
use crate::ftdi::{FtdiGpio, OutputState};
use crate::utils::parse_duration;

#[derive(Debug, Parser)]
pub struct ButtonCtrl {
    ///Button press duration (e.g., "1s", "500ms")
    #[arg(long, default_value = "1s", value_parser = parse_duration)]
    press_duration: Duration,
}

impl ButtonCtrl {
    pub async fn run(self) -> Result<()> {
        fn make_ftdi() -> Result<FtdiGpio> {
            FtdiGpio::builder()
                .with_default_device()
                .and_then(|b| b.configure())
                .wrap_err("failed to create ftdi device")
        }

        info!(
            "Holding button for {} seconds",
            self.press_duration.as_secs_f32()
        );
        tokio::task::spawn_blocking(move || -> Result<_, color_eyre::Report> {
            let mut ftdi = make_ftdi()?;
            ftdi.set_pin(BUTTON_PIN, OutputState::Low)?;
            std::thread::sleep(self.press_duration);
            ftdi.set_pin(BUTTON_PIN, OutputState::High)?;
            Ok(ftdi)
        })
        .await
        .wrap_err("task panicked")??;
        info!("Button released");

        Ok(())
    }
}
