use crate::ConeEvents;
use color_eyre::eyre;
use ftdi_embedded_hal::libftd2xx::{BitMode, Ft4232h, Ftdi, FtdiCommon};
use std::sync::mpsc;
use tracing::debug;

pub(crate) struct Button {}

const BUTTON_GPIO_PIN: u8 = 0;
const BUTTON_GPIO_MASK: u8 = 1 << BUTTON_GPIO_PIN;

impl Button {
    pub(crate) fn new(event_queue: mpsc::Sender<ConeEvents>) -> eyre::Result<Self> {
        let mut device: Ft4232h = Ftdi::with_index(7)?.try_into()?;
        let mask: u8 = !BUTTON_GPIO_MASK; // button pin as input, all others as output
        device.set_bit_mode(mask, BitMode::AsyncBitbang)?;
        debug!("Button GPIO initialized");

        // spawn a thread to poll the button
        std::thread::spawn(move || {
            loop {
                match device.bit_mode() {
                    Ok(mode) => {
                        // button is active low
                        let pressed = mode & BUTTON_GPIO_MASK == 0;
                        if let Err(e) =
                            event_queue.send(ConeEvents::ButtonPressed(pressed))
                        {
                            tracing::error!("Error sending event: {:?} - no receiver? stopping producer", e);
                            break;
                        }
                    }
                    Err(e) => {
                        tracing::error!("Error polling button: {:?}", e);
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        });

        Ok(Button {})
    }
}
