use clap::Parser;
use color_eyre::{eyre::WrapErr as _, Result};

use crate::ftdi::{FtdiChannel, FtdiId};

/// Reboot the orb
#[derive(Debug, Parser)]
pub struct Reboot {
    #[arg(short)]
    recovery: bool,

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

impl Reboot {
    pub async fn run(self) -> Result<()> {
        let device = match (self.usb_serial, self.ftdi_serial, self.desc) {
            (Some(usb_serial), None, None) => {
                Some(FtdiId::from_usb_serial(&usb_serial, self.channel))
            }
            (None, Some(ftdi_serial), None) => Some(FtdiId::FtdiSerial(ftdi_serial)),
            (None, None, Some(desc)) => Some(FtdiId::Description(desc)),
            (None, None, None) => None,
            _ => unreachable!(),
        };

        crate::boot::reboot(self.recovery, device.as_ref())
            .await
            .wrap_err_with(|| {
                format!(
                    "failed to reboot into {} mode",
                    if self.recovery { "recovery" } else { "normal" }
                )
            })
    }
}
