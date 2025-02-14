use clap::Parser;
use color_eyre::{eyre::WrapErr as _, Result};

use crate::ftdi::FtdiId;

#[derive(Debug, Parser)]
pub struct Reboot {
    #[arg(short)]
    recovery: bool,
    /// The serial number of the FTDI device to use
    #[arg(long, group = "id")]
    serial_num: Option<String>,
    /// The description of the FTDI device to use
    #[arg(long, group = "id")]
    desc: Option<String>,
    /// The index of the FTDI device to use
    #[arg(long, group = "id")]
    index: Option<u8>,
}

impl Reboot {
    pub async fn run(self) -> Result<()> {
        let device = match (self.serial_num, self.desc, self.index) {
            (Some(serial), None, None) => Some(FtdiId::SerialNumber(serial)),
            (None, Some(desc), None) => Some(FtdiId::Description(desc)),
            (None, None, Some(index)) => Some(FtdiId::Index(index)),
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
