pub mod button;
pub mod lcd;
pub mod led;

use crate::button::{Button, ButtonJoinHandle};
use crate::lcd::{Lcd, LcdJoinHandle};
use crate::led::{LedJoinHandle, LedStrip};
use color_eyre::eyre;
use color_eyre::eyre::Context;
use ftdi_embedded_hal::libftd2xx::{Ft4232h, Ftdi, FtdiCommon};
use tokio::sync::broadcast;

const CONE_FTDI_DEVICE_COUNT: usize = 8;

// FIXME: FTDI adapters aren't guaranteed to enumerate in the same order every time.
// Find a way to access a specific ftdi adapter with either a serial number of some other
// disambiguating info.
const CONE_FTDI_LCD_INDEX: i32 = 4;
const CONE_FTDI_LED_INDEX: i32 = 5;
const CONE_FTDI_BUTTON_INDEX: i32 = 7;

#[derive(Debug)]
#[allow(dead_code)]
enum Status {
    Connected,
    Disconnected,
}

pub struct ConeJoinHandle {
    pub lcd: LcdJoinHandle,
    pub led_strip: LedJoinHandle,
    button: ButtonJoinHandle,
}

impl ConeJoinHandle {
    pub async fn join(self) -> eyre::Result<()> {
        let (lcd_e, led_e, button_e) =
            tokio::try_join!(self.lcd.0, self.led_strip.0, self.button.0)?;

        // print any error that occurred
        if let Err(e) = lcd_e {
            tracing::error!("LCD task error: {:?}", e);
        }
        if let Err(e) = led_e {
            tracing::error!("LED task error: {:?}", e);
        }
        if let Err(e) = button_e {
            tracing::error!("Button task error: {:?}", e);
        }

        Ok(())
    }
}

/// Cone can be created only if connected to the host over USB.
pub struct Cone {
    pub lcd: Lcd,
    pub led_strip: LedStrip,
    _button: Button,
}

#[derive(Debug, Copy, Clone)]
pub enum ButtonState {
    Pressed,
    Released,
}

#[derive(Debug, Copy, Clone)]
pub enum ConeState {
    Connected,
    Disconnected,
}

#[derive(Debug, Copy, Clone)]
pub enum ConeEvent {
    Cone(ConeState),
    Button(ButtonState),
}

impl Cone {
    /// Create a new Cone instance.
    pub fn spawn(
        event_queue: broadcast::Sender<ConeEvent>,
    ) -> eyre::Result<(Self, ConeJoinHandle)> {
        if ftdi_embedded_hal::libftd2xx::list_devices()?.len() != CONE_FTDI_DEVICE_COUNT
        {
            return Err(eyre::eyre!(
                "FTDI device count mismatch: cone not connected?"
            ));
        } else {
            let mut device: Ft4232h = Ftdi::with_index(6)?
                .try_into()
                .wrap_err("Failed to initialize FTDI device")?;
            device.reset().wrap_err("Failed to reset")?;
        }

        let (lcd, lcd_handle) = Lcd::spawn()?;
        let (led_strip, led_handle) = LedStrip::spawn()?;
        let (button, button_handle) = Button::spawn(event_queue.clone())?;

        let cone = Cone {
            lcd,
            led_strip,
            _button: button,
        };

        let handle = ConeJoinHandle {
            lcd: lcd_handle,
            led_strip: led_handle,
            button: button_handle,
        };

        Ok((cone, handle))
    }
}
