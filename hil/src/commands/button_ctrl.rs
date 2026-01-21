use clap::Parser;
use color_eyre::{eyre::WrapErr as _, Result};
use humantime::parse_duration;
use std::time::Duration;
use tracing::{debug, info};

use crate::boot::{BUTTON_PIN, DEFAULT_CHANNEL};
use crate::ftdi::{FtdiChannel, FtdiGpio, OutputState};

/// Control the orb button over the debug board
#[derive(Debug, Parser)]
pub struct ButtonCtrl {
    /// Button press duration (e.g., "1s", "500ms")
    #[arg(long, default_value = "1s", value_parser = parse_duration)]
    press_duration: Duration,

    /// The USB serial number of the FTDI chip (e.g., "FT7ABC12").
    /// Will use the default channel (C) for this chip.
    #[arg(long, conflicts_with_all = ["ftdi_serial", "desc"])]
    usb_serial: Option<String>,

    /// The FTDI serial number including channel (e.g., "FT7ABC12C").
    /// This is the USB serial + channel letter (A/B/C/D).
    #[arg(long, conflicts_with_all = ["usb_serial", "desc"])]
    ftdi_serial: Option<String>,

    /// The FTDI description (e.g., "FT4232H C").
    #[arg(long, conflicts_with_all = ["usb_serial", "ftdi_serial"])]
    desc: Option<String>,

    /// The channel to use when --usb-serial is provided (A, B, C, or D).
    /// Defaults to C.
    #[arg(long, default_value = "C", requires = "usb_serial")]
    channel: FtdiChannel,
}

impl ButtonCtrl {
    pub async fn run(self) -> Result<()> {
        let usb_serial = self.usb_serial.clone();
        let ftdi_serial = self.ftdi_serial.clone();
        let desc = self.desc.clone();
        let channel = self.channel;

        let make_ftdi = move || -> Result<FtdiGpio> {
            let builder = FtdiGpio::builder();
            match (usb_serial.as_ref(), ftdi_serial.as_ref(), desc.as_ref()) {
                (Some(usb_serial), None, None) => {
                    debug!(
                        "using FTDI device with USB serial: {usb_serial}, channel: {:?}",
                        channel
                    );
                    builder
                        .with_usb_serial(usb_serial, channel)
                        .and_then(|b| b.configure())
                        .wrap_err("failed to create ftdi device with USB serial")
                }
                (None, Some(ftdi_serial), None) => {
                    debug!("using FTDI device with FTDI serial: {ftdi_serial}");
                    builder
                        .with_ftdi_serial(ftdi_serial)
                        .and_then(|b| b.configure())
                        .wrap_err("failed to create ftdi device with FTDI serial")
                }
                (None, None, Some(desc)) => {
                    debug!("using FTDI device with description: {desc}");
                    builder
                        .with_description(desc)
                        .and_then(|b| b.configure())
                        .wrap_err("failed to create ftdi device with description")
                }
                (None, None, None) => {
                    // Try default device first, fall back to default channel description
                    match builder.with_default_device() {
                        Ok(b) => {
                            b.configure().wrap_err("failed to configure ftdi device")
                        }
                        Err(e) => {
                            debug!("default device selection failed: {e}");
                            let desc_suffix = DEFAULT_CHANNEL.description_suffix();
                            debug!(
                                "attempting to find device with description '{desc_suffix}'"
                            );
                            FtdiGpio::builder()
                                .with_description(desc_suffix)
                                .and_then(|b| b.configure())
                                .wrap_err_with(|| {
                                    format!(
                                        "failed to open FTDI device with description \
                                         '{desc_suffix}'"
                                    )
                                })
                        }
                    }
                }
                _ => unreachable!(),
            }
        };

        info!(
            "Holding button for {} seconds",
            self.press_duration.as_secs_f32()
        );
        let press_duration = self.press_duration;
        tokio::task::spawn_blocking(move || -> Result<(), color_eyre::Report> {
            let mut ftdi = make_ftdi()?;
            ftdi.set_pin(BUTTON_PIN, OutputState::Low)?;
            std::thread::sleep(press_duration);
            ftdi.set_pin(BUTTON_PIN, OutputState::High)?;
            ftdi.destroy().wrap_err("failed to destroy ftdi")?;

            Ok(())
        })
        .await
        .wrap_err("task panicked")??;
        info!("Button released");

        Ok(())
    }
}
