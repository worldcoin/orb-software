//! USB HID relay implementation of the [`PinController`] trait.
//!
//! Controls a USB relay board via HID reports written to `/dev/hidrawN`.
//!
//! Protocol:
//! - Report: `[0x00, opcode, mask, 0, 0, 0, 0, 0, 0]`
//! - Opcode ON (close relay): `0xFF`
//! - Opcode OFF (open relay): `0xFD`
//! - Mask: bitmask for channels, channel N -> bit `(N - 1)`
//! - Device path: /dev/hidraw0`

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use color_eyre::{
    eyre::{ensure, WrapErr as _},
    Result,
};
use tracing::{debug};

use crate::orb::{BootMode, OrbManager};

const RELAY_ON: u8 = 0xFF;
const RELAY_OFF: u8 = 0xFD;

/// Identifies a single relay on a USB relay board.
#[derive(Debug, Clone)]
pub struct RelayChannel {
    /// Which relay board. Maps to `/dev/hidraw{X}`.
    pub bank: String,
    /// Which channel on that board (1..=8).
    pub channel: u32,
}

/// USB HID relay board controller implementing [`PinController`].
pub struct UsbRelay {
    power: RelayChannel,
    recovery: RelayChannel,
}

impl UsbRelay {
    pub fn new(power: RelayChannel, recovery: RelayChannel) -> Result<Self> {
        validate_channel(&power, "power")?;
        validate_channel(&recovery, "recovery")?;

        Ok(Self { power, recovery })
    }
}

fn validate_channel(ch: &RelayChannel, name: &str) -> Result<()> {
    ensure!(
        (1..=8).contains(&ch.channel),
        "{name} channel must be 1..=8, got {}",
        ch.channel
    );

    Ok(())
}

fn channel_to_mask(channel: u32) -> u8 {
    1u8 << (channel - 1)
}

fn write_relay_report(device: &Path, opcode: u8, mask: u8) -> Result<()> {
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

fn relay_on(ch: &RelayChannel) -> Result<()> {
    let device = PathBuf::from(ch.bank.clone());
    let mask = channel_to_mask(ch.channel);
    debug!(bank = ch.bank, channel = ch.channel, "relay ON");

    write_relay_report(&device, RELAY_ON, mask)
}

fn relay_off(ch: &RelayChannel) -> Result<()> {
    let device = PathBuf::from(ch.bank.clone());
    let mask = channel_to_mask(ch.channel);
    debug!(bank = ch.bank, channel = ch.channel, "relay OFF");

    write_relay_report(&device, RELAY_OFF, mask)
}

impl OrbManager for UsbRelay {
    fn press_power_button(&mut self, duration: Option<Duration>) -> Result<()> {
        relay_on(&self.power)?;

        if let Some(duration) = duration {
            std::thread::sleep(duration);
            relay_off(&self.power)?;
        }

        Ok(())
    }

    fn set_boot_mode(&mut self, mode: BootMode) -> Result<()> {
        match mode {
            BootMode::Recovery => relay_on(&self.recovery),
            BootMode::Normal => Ok(())
        }
    }

    fn hw_reset(&mut self) -> Result<()> {
        relay_off(&self.power)?;
        relay_off(&self.recovery)?;
        Ok(())
    }

    fn turn_off(&mut self) -> Result<()> {
        self.press_power_button(Some(Duration::from_secs(6)))
    }

    fn turn_on(&mut self) -> Result<()> {
        self.press_power_button(Some(Duration::from_secs(3)))
    }

    fn destroy(&mut self) -> Result<()> {
        Ok(())
    }
}
