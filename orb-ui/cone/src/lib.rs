pub mod button;
pub mod lcd;
pub mod led;

use crate::led::CONE_LED_COUNT;
use color_eyre::eyre;
use color_eyre::eyre::Context;
use orb_rgb::Argb;
use std::sync::mpsc;
use std::{env, fs};
use tinybmp::Bmp;

#[allow(dead_code)]
pub struct Cone {
    lcd: lcd::Lcd,
    led_strip_tx: mpsc::Sender<[Argb; CONE_LED_COUNT]>,
    button: button::Button,
    event_queue: mpsc::Sender<ConeEvents>,
}

pub struct ConeLeds(pub [Argb; CONE_LED_COUNT]);
pub struct ConeLcd(pub String);

pub enum ConeEvents {
    ButtonPressed(bool),
}

impl Cone {
    /// Create a new Cone instance.
    pub fn new(event_queue: mpsc::Sender<ConeEvents>) -> eyre::Result<Self> {
        let lcd = lcd::Lcd::new()?;
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
    pub fn queue_rgb_leds(&mut self, pixels: &ConeLeds) -> eyre::Result<()> {
        self.led_strip_tx
            .send(pixels.0)
            .wrap_err("Failed to send LED strip values")
    }

    pub fn lcd_load_image(&mut self, filepath: &str) -> eyre::Result<()> {
        // check if file exists, use absolute path for better understanding of the error
        let absolute_path = env::current_dir()?.join(filepath);
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
            let bmp_data = Bmp::from_slice(bmp_data.as_slice())
                .map_err(|e| eyre::eyre!("Error loading image: {:?}", e))?;
            self.lcd.load_bmp(&bmp_data)?;
        } else {
            return Err(eyre::eyre!(
                "File is not a .bmp image, format currently not supported: {:?}",
                absolute_path
            ));
        }

        Ok(())
    }

    pub fn lcd_test(&mut self) -> eyre::Result<()> {
        self.lcd.test()
    }
}
