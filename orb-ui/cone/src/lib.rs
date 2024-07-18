pub mod button;
pub mod lcd;
pub mod led;

use crate::led::CONE_LED_COUNT;
use color_eyre::eyre;
use color_eyre::eyre::Context;
use orb_rgb::Argb;
use std::{env, fs};
use tokio::sync::mpsc;

#[allow(dead_code)]
pub struct Cone {
    lcd: mpsc::UnboundedSender<Vec<u8>>,
    led_strip_tx: mpsc::UnboundedSender<[Argb; CONE_LED_COUNT]>,
    button: button::Button,
    event_queue: mpsc::UnboundedSender<ConeEvents>,
}

pub enum ConeEvents {
    ButtonPressed(bool),
}

impl Cone {
    /// Create a new Cone instance.
    pub fn new(event_queue: mpsc::UnboundedSender<ConeEvents>) -> eyre::Result<Self> {
        let lcd = lcd::Lcd::spawn()?;
        let led_strip_tx = led::Led::spawn()?;
        let button = button::Button::new(event_queue.clone())?;

        let cone = Cone {
            lcd,
            led_strip_tx,
            button,
            event_queue: event_queue.clone(),
        };

        Ok(cone)
    }

    /// Update the RGB LEDs by passing the values to the LED strip sender.
    pub fn queue_rgb_leds(
        &mut self,
        pixels: &[Argb; CONE_LED_COUNT],
    ) -> eyre::Result<()> {
        self.led_strip_tx
            .send(*pixels)
            .wrap_err("Failed to send LED strip values")
    }

    pub fn queue_lcd_raw(&mut self, image: Vec<u8>) -> eyre::Result<()> {
        self.lcd.send(image).wrap_err("Failed to send")
    }

    pub fn queue_lcd_bmp(&mut self, image: String) -> eyre::Result<()> {
        // check if file exists, use absolute path for better understanding of the error
        let absolute_path = env::current_dir()?.join(image);
        if !absolute_path.exists() {
            return Err(eyre::eyre!("File not found: {:?}", absolute_path));
        }

        // check if file is a bmp image
        if absolute_path
            .extension()
            .ok_or(eyre::eyre!("Unable to get file extension"))?
            == "bmp"
        {
            let bmp_data = fs::read(absolute_path)?;
            self.lcd.send(bmp_data).wrap_err("Failed to send")
        } else {
            Err(eyre::eyre!(
                "File is not a .bmp image, format currently not supported: {:?}",
                absolute_path
            ))
        }
    }
}
