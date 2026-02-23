use clap::Parser;
use color_eyre::{eyre::WrapErr as _, Result};

use crate::commands::PinCtrl;

/// Reboot the orb
#[derive(Debug, Parser)]
pub struct Reboot {
    #[arg(short)]
    recovery: bool,
    #[command(flatten)]
    pin_ctrl: PinCtrl,
}

impl Reboot {
    pub async fn run(self) -> Result<()> {
        let controller = tokio::task::spawn_blocking(move || {
            self.pin_ctrl
                .build_controller()
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
