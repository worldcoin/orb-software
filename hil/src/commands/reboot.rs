use clap::Parser;
use color_eyre::{eyre::WrapErr as _, Result};

use crate::orb::{orb_manager_from_config, OrbConfig};

/// Reboot the orb
#[derive(Debug, Parser)]
pub struct Reboot {
    #[arg(short)]
    recovery: bool,
    #[command(flatten)]
    orb_config: OrbConfig,
}

impl Reboot {
    pub async fn run(self) -> Result<()> {
        let orb_config = self.orb_config.use_file_if_exists()?;

        let controller = tokio::task::spawn_blocking(move || {
            orb_manager_from_config(&orb_config)
                .wrap_err("failed to create pin controller")
        })
        .await
        .wrap_err("task panicked")??;

        crate::boot::reboot(self.recovery, controller)
            .await
            .wrap_err_with(|| {
                format!(
                    "failed to reboot into {} mode",
                    if self.recovery { "recovery" } else { "normal" }
                )
            })
    }
}
