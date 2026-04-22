use clap::Parser;
use color_eyre::{eyre::eyre, eyre::WrapErr as _, Result};
use std::num::NonZeroU8;
use tokio::time::Duration;
use tracing::{info, warn};

use crate::{orb_manager_from_config, OrbConfig};

/// Reboot the orb
#[derive(Debug, Parser)]
pub struct Reboot {
    #[arg(short)]
    recovery: bool,
    #[arg(short, long, default_value_t = true, action = clap::ArgAction::Set)]
    make_sure: bool,
    #[arg(short, long, default_value_t = NonZeroU8::new(1).unwrap())]
    attempts_count: NonZeroU8,
}

impl Reboot {
    pub async fn run(self, orb_config: &OrbConfig) -> Result<()> {
        let mut controller = tokio::task::block_in_place(|| {
            orb_manager_from_config(orb_config)
                .wrap_err("failed to create pin controller")
        })?;

        let orb_mode = if self.recovery { "recovery" } else { "normal" };

        for i in 1..=self.attempts_count.into() {
            if let Err(e) =
                crate::boot::reboot(self.recovery, controller.as_mut()).await
            {
                warn!("Attempt {}, cannot reboot: {}", i, e);
                controller = tokio::task::block_in_place(|| {
                    orb_manager_from_config(orb_config)
                        .wrap_err("failed to create pin controller")
                })?;
                continue;
            }

            if !self.make_sure {
                return Ok(());
            }

            // some time is required to get into recovery
            tokio::time::sleep(Duration::from_secs(5)).await;

            match crate::boot::is_recovery_mode_detected().await {
                Err(e) => {
                    warn!(
                        "Attempt {}, cannot get into {} mode because of error: {}",
                        i, orb_mode, e
                    );
                }
                Ok(is_in_rcm) => {
                    if is_in_rcm != self.recovery {
                        warn!("Attempt {}, cannot get into {} mode", i, orb_mode);
                    } else {
                        info!("Attempt {}, got into {} mode", i, orb_mode);
                        return Ok(());
                    }
                }
            }
        }

        Err(eyre!(
            "Cannot get into {} mode with {} attempts",
            orb_mode,
            self.attempts_count
        ))
    }
}
