//! USB relay implementations of [`OrbManager`].
//!
//! Supports two relay protocols behind a common [`Relay`] type:
//!
//! **USB HID** (`/dev/hidrawN`):
//! - Report: `[0x00, opcode, mask, 0, 0, 0, 0, 0, 0]`
//! - Opcode ON (close relay): `0xFF`; OFF (open relay): `0xFD`
//! - Mask: bitmask for channels, channel N → bit `(N - 1)` (1-indexed, 1..=8)
//!
//! **Numato USB serial** (`/dev/ttyACMN`, 9600 baud):
//! - Turn on channel N:  `relay on N\r`
//! - Turn off channel N: `relay off N\r`
//! - Channels are 0-indexed (0..=7 for an 8-channel board)

use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use color_eyre::{
    eyre::{ensure, WrapErr as _},
    Result,
};
use tracing::debug;

use crate::orb::{BootMode, OrbManager};

const HID_ON: u8 = 0xFF;
const HID_OFF: u8 = 0xFD;

enum RelayDriver {
    UsbHid { bank: PathBuf },
    Numato { bank: PathBuf },
}

impl RelayDriver {
    fn close_channel(&self, channel: u32) -> Result<()> {
        match self {
            Self::UsbHid { bank } => {
                let mask = 1u8 << (channel - 1);
                debug!(channel, "usb hid relay ON");
                write_hid_report(bank, HID_ON, mask)
            }
            Self::Numato { bank } => {
                debug!(channel, "numato relay ON");
                let cmd = format!("relay on {channel}\r");
                write_serial_cmd(bank, &cmd)
            }
        }
    }

    fn open_channel(&self, channel: u32) -> Result<()> {
        match self {
            Self::UsbHid { bank } => {
                let mask = 1u8 << (channel - 1);
                debug!(channel, "usb hid relay OFF");
                write_hid_report(bank, HID_OFF, mask)
            }
            Self::Numato { bank } => {
                debug!(channel, "numato relay OFF");
                let cmd = format!("relay off {channel}\r");
                write_serial_cmd(bank, &cmd)
            }
        }
    }
}

pub struct Relay {
    driver: RelayDriver,
    power: u32,
    recovery: u32,
    off_duration: Duration,
    on_duration: Duration,
}

impl Relay {
    pub fn new_usb_hid(
        bank: &str,
        power: u32,
        recovery: u32,
        off_duration: Duration,
        on_duration: Duration,
    ) -> Result<Self> {
        ensure!(
            (1..=8).contains(&power),
            "usb hid power channel must be 1..=8, got {power}"
        );
        ensure!(
            (1..=8).contains(&recovery),
            "usb hid recovery channel must be 1..=8, got {recovery}"
        );

        Ok(Self {
            driver: RelayDriver::UsbHid {
                bank: PathBuf::from(bank),
            },
            power,
            recovery,
            off_duration,
            on_duration,
        })
    }

    pub fn new_numato(
        device_path: &str,
        power: u32,
        recovery: u32,
        off_duration: Duration,
        on_duration: Duration,
    ) -> Result<Self> {
        ensure!(
            (0..=7).contains(&power),
            "numato power channel must be 0..=7, got {power}"
        );
        ensure!(
            (0..=7).contains(&recovery),
            "numato recovery channel must be 0..=7, got {recovery}"
        );

        Ok(Self {
            driver: RelayDriver::Numato {
                bank: PathBuf::from(device_path),
            },
            power,
            recovery,
            off_duration,
            on_duration,
        })
    }
}

impl OrbManager for Relay {
    fn press_power_button(&mut self, duration: Option<Duration>) -> Result<()> {
        let ch = self.power;
        self.driver.close_channel(ch)?;

        if let Some(duration) = duration {
            std::thread::sleep(duration);
            self.driver.open_channel(ch)?;
        }

        Ok(())
    }

    fn set_boot_mode(&mut self, mode: BootMode) -> Result<()> {
        match mode {
            BootMode::Recovery => self.driver.close_channel(self.recovery),
            BootMode::Normal => self.driver.open_channel(self.recovery),
        }
    }

    fn hw_reset(&mut self) -> Result<()> {
        self.driver.open_channel(self.power)?;
        self.driver.open_channel(self.recovery)?;

        Ok(())
    }

    fn turn_off(&mut self) -> Result<()> {
        self.press_power_button(Some(self.off_duration))
    }

    fn turn_on(&mut self) -> Result<()> {
        self.press_power_button(Some(self.on_duration))
    }

    fn destroy(&mut self) -> Result<()> {
        Ok(())
    }
}

fn write_hid_report(device: &Path, opcode: u8, mask: u8) -> Result<()> {
    let mut f = OpenOptions::new()
        .write(true)
        .open(device)
        .wrap_err_with(|| format!("cannot open relay device: {}", device.display()))?;

    let report = [0x00u8, opcode, mask, 0, 0, 0, 0, 0, 0];
    f.write_all(&report).wrap_err_with(|| {
        format!("failed writing HID report to {}", device.display())
    })?;

    Ok(())
}

fn write_serial_cmd(device: &Path, cmd: &str) -> Result<()> {
    let mut f = OpenOptions::new()
        .write(true)
        .open(device)
        .wrap_err_with(|| format!("cannot open numato relay: {}", device.display()))?;

    f.write_all(cmd.as_bytes())
        .wrap_err_with(|| format!("failed writing command to {}", device.display()))?;

    Ok(())
}
