use clap::Args;
use color_eyre::{eyre::bail, Result};

// Re-export types from ftdi module for convenience
pub use crate::ftdi::{FtdiGpio, FtdiParams};

/// Parameters for creating a pin controller.
#[derive(Debug, Clone, Args)]
pub struct PinCtrl {
    /// Type of pin controller to use (ftdi, relay, mock)
    #[arg(long, default_value = "ftdi", value_name = "TYPE")]
    pub pin_ctrl_type: String,

    #[command(flatten)]
    pub ftdi: FtdiParams,
    // Future: add other controller types here
    // #[command(flatten)]
    // pub relay: RelayParams,
}

impl PinCtrl {
    /// Build a pin controller from the parameters.
    ///
    /// This is a blocking operation that initializes the hardware controller.
    /// Returns a trait object for runtime polymorphism.
    pub fn build_controller(self) -> Result<Box<dyn PinController + Send>> {
        match self.pin_ctrl_type.as_str() {
            "ftdi" => {
                let ftdi = FtdiGpio::builder().with_params(self.ftdi)?.configure()?;
                Ok(Box::new(ftdi))
            }
            "relay" => {
                bail!("Relay pin controller not yet implemented")
            }
            "mock" => {
                bail!("Mock pin controller not yet implemented")
            }
            other => {
                bail!("Unknown pin controller type: '{}'. Supported types: ftdi, relay, mock", other)
            }
        }
    }
}

/// Trait for controlling power and recovery pins on hardware devices.
///
/// This trait provides a high-level interface for controlling the Orb's
/// power button and recovery mode pins. Different hardware backends
/// (FTDI, relay, GPIO sysfs, mock for testing, etc.) can implement this
/// trait to provide the same functionality.
pub trait PinController {
    /// Press the power button for the specified duration.
    ///
    /// If duration is None, the button remains pressed (caller must ensure it's released).
    /// If duration is Some, the button is pressed for that duration then released.
    fn press_power_button(
        &mut self,
        duration: Option<std::time::Duration>,
    ) -> Result<()>;

    /// Control the recovery mode pin.
    ///
    /// - `true`: Recovery mode enabled (device boots into recovery)
    /// - `false`: Normal boot mode
    fn set_recovery(&mut self, enabled: bool) -> Result<()>;

    /// Reset the controller hardware state.
    ///
    /// This resets the controller to a clean state, typically resetting all
    /// pins to their default values and reinitializing the hardware interface.
    /// This is important between power cycles to ensure pins don't remain in
    /// their previous state.
    fn reset(&mut self) -> Result<()>;

    /// Turn off the device by pressing the power button.
    fn turn_off(&mut self) -> Result<()>;

    /// Turn on the device by pressing the power button.
    fn turn_on(&mut self) -> Result<()>;
}
