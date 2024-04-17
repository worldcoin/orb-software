pub mod button;
pub mod lcd;
pub mod led;

use crate::led::{Argb, CONE_LED_COUNT};
use color_eyre::eyre;
use std::sync::mpsc;
use std::{env, fs};

#[allow(dead_code)]
pub struct Cone {
    lcd: lcd::Lcd,
    led_strip: led::Led,
    button: button::Button,
    event_queue: mpsc::Sender<ConeEvents>,
}

pub enum ConeEvents {
    ButtonPressed(bool),
}

impl Cone {
    /// Create a new Cone instance.
    pub fn new(event_queue: mpsc::Sender<ConeEvents>) -> eyre::Result<Self> {
        let lcd = lcd::Lcd::new()?;
        let led_strip = led::Led::new()?;
        let button = button::Button::new(event_queue.clone())?;

        let cone = Cone {
            lcd,
            led_strip,
            button,
            event_queue: event_queue.clone(),
        };

        Ok(cone)
    }

    pub fn leds_update_rgb(
        &mut self,
        pixels: &[Argb; CONE_LED_COUNT],
    ) -> eyre::Result<()> {
        self.led_strip.spi_rgb_led_update_rgb(pixels)
    }

    pub fn lcd_load_image(&mut self, filepath: &str) -> eyre::Result<()> {
        // check if file exists, use absolute path for better understanding of the error
        let absolute_path = env::current_dir()?.join(filepath);
        if !absolute_path.exists() {
            return Err(eyre::eyre!("File not found: {:?}", absolute_path));
        }

        // load bmp image to bytes
        let bmp_data = fs::read(absolute_path)?;
        self.lcd.load_image(bmp_data.as_slice())?;

        Ok(())
    }

    pub fn lcd_test(&mut self) -> eyre::Result<()> {
        self.lcd.test()
    }
}
