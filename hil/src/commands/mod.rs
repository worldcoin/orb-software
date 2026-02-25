mod button_ctrl;
mod cmd;
mod fetch_persistent;
mod flash;
mod login;
mod mcu;
mod nfsboot;
mod ota;
mod reboot;
mod set_recovery_pin;

pub use self::button_ctrl::ButtonCtrl;
pub use self::cmd::Cmd;
pub use self::fetch_persistent::FetchPersistent;
pub use self::flash::Flash;
pub use self::login::Login;
pub use self::mcu::Mcu;
pub use self::nfsboot::Nfsboot;
pub use self::ota::Ota;
pub use self::reboot::Reboot;
pub use self::set_recovery_pin::SetRecoveryPin;

use clap::Args;
use color_eyre::{eyre::bail, Result};

use crate::ftdi::FtdiGpio;
use crate::relay::{RelayChannel, UsbRelay};
use orb_hil::pin_controller::PinController;

/// Parameters for creating a pin controller.
#[derive(Debug, Clone, Args)]
pub struct PinCtrl {
    /// Type of pin controller to use (ftdi, relay)
    #[arg(long, default_value = "ftdi", value_name = "TYPE")]
    pub pin_ctrl_type: String,

    /// FTDI device serial number
    #[arg(long)]
    pub ftdi_serial_number: Option<String>,

    /// FTDI device description
    #[arg(long)]
    pub ftdi_description: Option<String>,

    /// Relay bank for the power button channel (1-indexed)
    #[arg(long, default_value_t = 0)]
    pub relay_power_bank: u32,

    /// Relay channel for recovery-mode
    #[arg(long, default_value_t = 1)]
    pub relay_recovery_channel: u32,

    /// Relay channel for the power button
    #[arg(long, default_value_t = 2)]
    pub relay_power_channel: u32,
}

impl PinCtrl {
    pub fn build_controller(self) -> Result<Box<dyn PinController + Send>> {
        match self.pin_ctrl_type.as_str() {
            "ftdi" => {
                let builder = FtdiGpio::builder();
                let configured = match (self.ftdi_serial_number, self.ftdi_description)
                {
                    (Some(serial), _) => builder.with_serial_number(&serial)?,
                    (None, Some(desc)) => builder.with_description(&desc)?,
                    (None, None) => builder.with_default_device()?,
                };
                Ok(Box::new(configured.configure()?))
            }
            "relay" => {
                let power = RelayChannel {
                    bank: self.relay_power_bank,
                    channel: self.relay_power_channel,
                };
                let recovery = RelayChannel {
                    bank: self.relay_power_bank,
                    channel: self.relay_recovery_channel,
                };
                Ok(Box::new(UsbRelay::new(power, recovery)?))
            }
            other => {
                bail!(
                    "Unknown pin controller type: '{}'. \
                     Supported types: ftdi, relay",
                    other
                )
            }
        }
    }
}
