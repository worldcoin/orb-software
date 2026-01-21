use std::time::Duration;

use crate::ftdi::{FtdiChannel, FtdiGpio, FtdiId, OutputState};
use color_eyre::{eyre::WrapErr as _, Result};
use tracing::{debug, info};

pub const BUTTON_PIN: crate::ftdi::Pin = FtdiGpio::DTR_PIN;
pub const RECOVERY_PIN: crate::ftdi::Pin = FtdiGpio::CTS_PIN;
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

/// The default channel used for button/recovery control on the HIL.
pub const DEFAULT_CHANNEL: FtdiChannel = FtdiChannel::C;

/// If `device` is `None`, will get the first available device, or fall back to
/// the default channel (FT4232H C) if multiple devices are found.
#[tracing::instrument]
pub async fn reboot(recovery: bool, device: Option<&FtdiId>) -> Result<()> {
    fn make_ftdi(device: Option<FtdiId>) -> Result<FtdiGpio> {
        let builder = FtdiGpio::builder();
        let builder = match &device {
            Some(FtdiId::Description(desc)) => builder.with_description(desc),
            Some(FtdiId::FtdiSerial(serial)) => builder.with_ftdi_serial(serial),
            None => match builder.with_default_device() {
                Ok(b) => return b.configure().wrap_err("failed to configure ftdi"),
                Err(e) => {
                    tracing::error!("failed to build default ftdi: {}, trying with default channel name", e);
                    // Fall back to default channel when multiple devices exist
                    return FtdiGpio::builder()
                        .with_description(DEFAULT_CHANNEL.description_suffix())
                        .and_then(|b| b.configure())
                        .wrap_err("failed to create ftdi device with default channel");
                }
            },
        };
        builder
            .and_then(|b| b.configure())
            .wrap_err("failed to create ftdi device")
    }

    info!("Turning off");
    let device_clone = device.cloned();
    let ftdi = tokio::task::spawn_blocking(|| -> Result<_, color_eyre::Report> {
        for d in FtdiGpio::list_devices().wrap_err("failed to list ftdi devices")? {
            debug!(
                "ftdi device: desc:{}, serial:{}, vid:{}, pid:{}",
                d.description, d.serial_number, d.vendor_id, d.product_id,
            );
        }
        let mut ftdi = make_ftdi(device_clone)?;
        ftdi.set_pin(BUTTON_PIN, OutputState::Low)?;
        ftdi.set_pin(RECOVERY_PIN, OutputState::High)?;
        Ok(ftdi)
    })
    .await
    .wrap_err("task panicked")??;
    tokio::time::sleep(Duration::from_secs(10)).await;

    info!("Resetting FTDI");
    tokio::task::spawn_blocking(move || ftdi.destroy())
        .await
        .wrap_err("task panicked")??;
    tokio::time::sleep(Duration::from_secs(4)).await;

    info!("Turning on");
    let device_clone = device.cloned();
    let ftdi = tokio::task::spawn_blocking(move || -> Result<_, color_eyre::Report> {
        let mut ftdi = make_ftdi(device_clone)?;
        let recovery_state = if recovery {
            OutputState::Low
        } else {
            OutputState::High
        };
        ftdi.set_pin(BUTTON_PIN, OutputState::Low)?;
        ftdi.set_pin(RECOVERY_PIN, recovery_state)?;
        Ok(ftdi)
    })
    .await
    .wrap_err("task panicked")??;
    tokio::time::sleep(Duration::from_secs(4)).await;

    tokio::task::spawn_blocking(move || ftdi.destroy())
        .await
        .wrap_err("task panicked")??;
    tokio::time::sleep(Duration::from_secs(1)).await;
    info!("Done triggering reboot");

    Ok(())
}
