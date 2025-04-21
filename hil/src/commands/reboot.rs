use clap::Parser;
use color_eyre::{eyre::WrapErr as _, Result};

use crate::ftdi::FtdiId;

/// Reboot the orb
#[derive(Debug, Parser)]
pub struct Reboot {
    #[arg(short)]
    recovery: bool,
    /// The serial number of the FTDI device to use
    #[arg(long, conflicts_with = "desc")]
    serial_num: Option<String>,
    /// The description of the FTDI device to use
    #[arg(long, conflicts_with = "serial_num")]
    desc: Option<String>,
}

impl Reboot {
    pub async fn run(self) -> Result<()> {
        let device = match (self.serial_num, self.desc) {
            (Some(serial), None) => Some(FtdiId::SerialNumber(serial)),
            (None, Some(desc)) => Some(FtdiId::Description(desc)),
            (None, None) => None,
            (Some(_), Some(_)) => unreachable!(),
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
