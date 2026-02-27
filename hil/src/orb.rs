use clap::{arg, Args, ValueEnum};
use color_eyre::{eyre::bail, Result};
use serde::Deserialize;
use std::fmt;

use crate::ftdi::FtdiGpio;

/// Orb platform type
#[derive(Debug, Clone, Copy, ValueEnum, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Diamond,
    Pearl,
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Platform::Diamond => write!(f, "diamond"),
            Platform::Pearl => write!(f, "pearl"),
        }
    }
}

#[derive(Default, Debug, Clone, Copy, ValueEnum, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PinControlType {
    #[default]
    Ftdi,
    UsbRelay,
}

/// Configuration for the orb, including pin controller and serial path.
#[derive(Debug, Clone, Args, Deserialize, bon::Builder)]
pub struct OrbConfig {
    /// Path to YAML config file for orb configuration. If provided, other
    /// arguments are ignored.
    #[arg(long)]
    #[serde(skip)]
    pub orb_config_path: Option<std::path::PathBuf>,

    /// Orb identifier (e.g., serial number or name)
    #[arg(long)]
    pub orb_id: Option<String>,

    #[arg(long)]
    pub hostname: Option<String>,

    /// Platform type (diamond or pearl)
    #[arg(long, value_enum)]
    pub platform: Option<Platform>,

    /// Path to the serial device (e.g., /dev/ttyUSB0)
    #[arg(long)]
    pub serial_path: Option<std::path::PathBuf>,

    /// Type of pin controller to use (ftdi, relay)
    #[arg(long, value_enum, default_value_t = PinControlType::Ftdi)]
    #[serde(default)]
    pub pin_ctrl_type: PinControlType,

    /// FTDI device serial number
    #[arg(long)]
    pub serial_num: Option<String>,

    /// FTDI device description
    #[arg(long)]
    pub desc: Option<String>,
}

impl OrbConfig {
    pub fn use_file_if_exists(&self) -> Result<OrbConfig> {
        if let Some(config_path) = &self.orb_config_path {
            let file = std::fs::File::open(config_path)?;
            let config: OrbConfig = serde_yaml::from_reader(file)?;
            Ok(config)
        } else {
            Ok(self.clone())
        }
    }

    /// Creates a hostname from the orb_id by prepending "orb-".
    /// Returns None if orb_id is not set.
    pub fn get_hostname(&self) -> Option<String> {
        if self.hostname.is_some() {
            return self.hostname.clone();
        }
        self.orb_id.as_ref().map(|id| format!("orb-{}.local", id))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootMode {
    Normal,
    Recovery,
}

/// Trait for controlling power and recovery pins on hardware devices.
pub trait OrbManager {
    /// Press the power button for the specified duration.
    ///
    /// If duration is None, the button remains pressed (caller must ensure it's released).
    /// If duration is Some, the button is pressed for that duration then released.
    fn press_power_button(
        &mut self,
        duration: Option<std::time::Duration>,
    ) -> Result<()>;

    /// Set the boot mode for the device.
    ///
    /// - `BootMode::Recovery`: Device boots into recovery mode
    /// - `BootMode::Normal`: Device boots normally
    fn set_boot_mode(&mut self, mode: BootMode) -> Result<()>;

    /// Perform a hardware reset of the controller.
    ///
    /// This fully resets the FTDI hardware and re-initializes it.
    /// After reset, all pins are set to HIGH (safe/released state).
    fn hw_reset(&mut self) -> Result<()>;

    /// Turn off the device by pressing the power button.
    fn turn_off(&mut self) -> Result<()>;

    /// Turn on the device by pressing the power button.
    fn turn_on(&mut self) -> Result<()>;

    /// Destroy the controller, resetting hardware state.
    fn destroy(&mut self) -> Result<()>;
}

pub fn orb_manager_from_config(
    config: &OrbConfig,
) -> Result<Box<dyn OrbManager + Send>> {
    match config.pin_ctrl_type {
        PinControlType::Ftdi => {
            let builder = FtdiGpio::builder();
            let configured = match (config.serial_num.as_ref(), config.desc.as_ref()) {
                (Some(serial), _) => builder.with_serial_number(serial)?,
                (None, Some(desc)) => builder.with_description(desc)?,
                (None, None) => builder.with_default_device()?,
            };
            Ok(Box::new(configured.configure()?))
        }
        PinControlType::UsbRelay => {
            bail!("Relay pin controller not yet implemented")
        }
    }
}
