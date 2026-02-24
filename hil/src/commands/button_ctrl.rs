use clap::Parser;
use color_eyre::{eyre::WrapErr as _, Result};
use humantime::parse_duration;
use std::time::Duration;
use tracing::info;

use crate::commands::PinCtrl;

#[derive(Debug, Parser)]
pub struct ButtonCtrl {
    ///Button press duration (e.g., "1s", "500ms")
    #[arg(long, default_value = "1s", value_parser = parse_duration)]
    press_duration: Duration,
    #[command(flatten)]
    pub pin_ctrl: PinCtrl,
}

impl ButtonCtrl {
    pub async fn run(self) -> Result<()> {
        info!(
            "Holding button for {} seconds",
            self.press_duration.as_secs_f32()
        );

        tokio::task::spawn_blocking(move || -> Result<(), color_eyre::Report> {
            let mut controller = self
                .pin_ctrl
                .build_controller()
                .wrap_err("failed to create pin controller")?;

            controller.press_power_button(Some(self.press_duration))?;

            controller
                .destroy()
                .wrap_err("failed to destroy pin controller")?;
            Ok(())
        })
        .await
        .wrap_err("task panicked")??;

        info!("Button released");

        Ok(())
    }
}
