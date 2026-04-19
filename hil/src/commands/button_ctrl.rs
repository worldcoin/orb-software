use clap::Parser;
use color_eyre::{eyre::WrapErr as _, Result};
use humantime::parse_duration;
use std::time::Duration;
use tracing::info;

use crate::{orb_manager_from_config, OrbConfig};

#[derive(Debug, Parser)]
pub struct ButtonCtrl {
    ///Button press duration (e.g., "1s", "500ms")
    #[arg(long, default_value = "1s", value_parser = parse_duration)]
    press_duration: Duration,
}

impl ButtonCtrl {
    pub async fn run(self, orb_config: &OrbConfig) -> Result<()> {
        info!(
            "Holding button for {} seconds",
            self.press_duration.as_secs_f32()
        );

        let orb_config = orb_config.clone();
        tokio::task::spawn_blocking(move || -> Result<(), color_eyre::Report> {
            let mut orb_mgr = orb_manager_from_config(&orb_config)
                .wrap_err("failed to create pin controller")?;

            orb_mgr.press_power_button(Some(self.press_duration))?;

            orb_mgr
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
