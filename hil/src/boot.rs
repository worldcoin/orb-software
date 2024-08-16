use std::{path::Path, time::Duration};

use crate::{
    ftdi::{FtdiGpio, OutputState},
    serial::wait_for_login_prompt,
};
use color_eyre::{eyre::WrapErr as _, Result};
use futures::{StreamExt, TryStreamExt};
use tokio_serial::SerialPortBuilderExt;
use tracing::{info, warn};

const BUTTON_PIN: crate::ftdi::Pin = FtdiGpio::CTS_PIN;
const RECOVERY_PIN: crate::ftdi::Pin = FtdiGpio::RTS_PIN;
const ORB_BAUD_RATE: u32 = 115200;
const NVIDIA_VENDOR_ID: u16 = 0x0955;

pub fn is_recovery_mode_detected() -> Result<bool> {
    let num_nvidia_devices = nusb::list_devices()
        .wrap_err("failed to enumerate usb devices")?
        .filter(|d| d.vendor_id() == NVIDIA_VENDOR_ID)
        .count();
    Ok(num_nvidia_devices > 0)
}

// Note: we are calling some blocking code from async here, but its probably fine.
#[tracing::instrument]
pub async fn reboot(recovery: bool, wait_for_login: Option<&Path>) -> Result<()> {
    fn make_ftdi() -> Result<FtdiGpio> {
        FtdiGpio::builder()
            .with_default_device()
            .and_then(|b| b.configure())
            .wrap_err("failed to create ftdi device")
    }
    info!("Turning off");
    let ftdi = tokio::task::spawn_blocking(|| -> Result<_, color_eyre::Report> {
        let mut ftdi = make_ftdi()?;
        ftdi.set_pin(BUTTON_PIN, OutputState::Low)?;
        ftdi.set_pin(RECOVERY_PIN, OutputState::High)?;
        Ok(ftdi)
    })
    .await
    .wrap_err("task panicked")??;
    tokio::time::sleep(Duration::from_secs(10)).await;

    info!("Resetting FTDI");
    ftdi.destroy().wrap_err("failed to destroy ftdi")?;
    tokio::time::sleep(Duration::from_secs(4)).await;

    info!("Turning on");
    let ftdi = tokio::task::spawn_blocking(move || -> Result<_, color_eyre::Report> {
        let mut ftdi = make_ftdi()?;
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

    ftdi.destroy().wrap_err("failed to destroy ftdi")?;
    tokio::time::sleep(Duration::from_secs(1)).await;
    info!("Done triggering reboot");

    let Some(serial_path) = wait_for_login else {
        return Ok(());
    };
    let serial = tokio_serial::new(serial_path.to_string_lossy(), ORB_BAUD_RATE)
        .open_native_async()
        .wrap_err_with(|| {
            format!("failed to open serial port {}", serial_path.display())
        })?;

    let (serial_tx, serial_rx) = tokio::sync::broadcast::channel(64);
    let _serial_join_handle = tokio::task::spawn(async move {
        let mut serial_stream = tokio_util::io::ReaderStream::new(serial);
        while let Some(chunk) = serial_stream
            .try_next()
            .await
            .wrap_err("error reading from serial")?
        {
            if let Err(_err) = serial_tx.send(chunk) {
                warn!("dropping serial data due to slow receivers. consider a larger channel size");
            }
        }
        Ok::<(), color_eyre::Report>(())
    });
    let serial_rx = tokio_stream::wrappers::BroadcastStream::new(serial_rx)
        .map(|result| result.expect("todo"));

    wait_for_login_prompt(serial_rx)
        .await
        .wrap_err("failed to wait for login prompt")?;

    Ok(())
}
