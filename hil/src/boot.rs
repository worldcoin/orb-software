use std::time::Duration;

use crate::orb::{BootMode, OrbManager};
use color_eyre::{eyre::WrapErr as _, Result};
use tracing::info;

pub const NVIDIA_VENDOR_ID: u16 = 0x0955;
pub const NVIDIA_USB_ETHERNET: u16 = 0x7035;

pub async fn is_recovery_mode_detected() -> Result<bool> {
    let num_nvidia_devices = nusb::list_devices()
        .await
        .wrap_err("failed to enumerate usb devices")?
        .filter(|d| {
            d.vendor_id() == NVIDIA_VENDOR_ID && d.product_id() != NVIDIA_USB_ETHERNET
        })
        .count();
    Ok(num_nvidia_devices > 0)
}

/// Reboot the device using a pin controller.
///
/// The controller's reset() method is called between power-off and power-on
/// to ensure pins return to their default state.
#[tracing::instrument(skip(controller))]
pub async fn reboot(
    recovery: bool,
    mut controller: Box<dyn OrbManager + Send>,
) -> Result<()> {
    tokio::task::spawn_blocking(move || -> Result<(), color_eyre::Report> {
        info!("Turning off");
        controller.set_boot_mode(BootMode::Normal)?;
        controller.turn_off()?;

        // Hardware reset controller to default state
        info!("Performing hardware reset");
        controller.hw_reset()?;

        std::thread::sleep(Duration::from_secs(4));

        info!("Turning on");
        let mode = if recovery {
            BootMode::Recovery
        } else {
            BootMode::Normal
        };
        controller.set_boot_mode(mode)?;
        controller.turn_on()?;

        controller
            .destroy()
            .wrap_err("failed to destroy pin controller")?;

        info!("Done triggering reboot");

        Ok(())
    })
    .await
    .wrap_err("task panicked")??;

    Ok(())
}
