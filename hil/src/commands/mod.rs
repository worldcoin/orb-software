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

use crate::ftdi::{FtdiGpio, FtdiId};
use crate::pin_controller::PinController;

/// Parameters for creating a pin controller.
#[derive(Debug, Clone, Args)]
pub struct PinCtrl {
    /// Type of pin controller to use (ftdi, relay, mock)
    #[arg(long, default_value = "ftdi", value_name = "TYPE")]
    pub pin_ctrl_type: String,

    /// FTDI device serial number
    #[arg(long)]
    pub ftdi_serial_number: Option<String>,

    /// FTDI device description
    #[arg(long)]
    pub ftdi_description: Option<String>,
}

impl PinCtrl {
    pub fn build_controller(self) -> Result<Box<dyn PinController + Send>> {
        match self.pin_ctrl_type.as_str() {
            "ftdi" => {
                let builder = FtdiGpio::builder();
                let configured = match (self.ftdi_serial_number, self.ftdi_description) {
                    (Some(serial), _) => builder.with_id(FtdiId::SerialNumber(serial))?,
                    (None, Some(desc)) => builder.with_id(FtdiId::Description(desc))?,
                    (None, None) => builder.with_default_device()?,
                };
                Ok(Box::new(configured.configure()?))
            }
            "relay" => {
                bail!("Relay pin controller not yet implemented")
            }
            "mock" => {
                bail!("Mock pin controller not yet implemented")
            }
            other => {
                bail!(
                    "Unknown pin controller type: '{}'. Supported types: ftdi, relay, mock",
                    other
                )
            }
        }
    }
}
