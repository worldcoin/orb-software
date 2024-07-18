pub mod button;
pub mod lcd;
pub mod led;

use crate::led::CONE_LED_COUNT;
use color_eyre::eyre;
use color_eyre::eyre::Context;
use image::{ImageFormat, Luma};
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

    /// Update the LCD screen with a QR code.
    /// `qr_str` is encoded as a QR code and sent to the LCD screen.
    pub fn queue_lcd_qr_code(&mut self, qr_str: String) -> eyre::Result<()> {
        let qr_code = qrcode::QrCode::new(qr_str.as_bytes())?
            .render::<Luma<u8>>()
            .dark_color(Luma([255u8])) // invert color: black background, white QR code
            .light_color(Luma([0]))
            .quiet_zone(true) // disable quiet zone (white border)
            .min_dimensions(200, 200)
            .max_dimensions(230, 230) // sets maximum image size
            .build();
        let mut buffer = std::io::Cursor::new(vec![]);
        qr_code.write_to(&mut buffer, ImageFormat::Bmp)?;
        tracing::debug!("LCD QR: {:?}", qr_str);
        self.lcd
            .send(buffer.into_inner())
            .wrap_err("Failed to send")
    }

    /// Update the LCD screen with a BMP image.
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
            tracing::debug!("LCD image: {:?}", absolute_path);
            let bmp_data = fs::read(absolute_path)?;
            self.lcd.send(bmp_data).wrap_err("Failed to send")
        } else {
            Err(eyre::eyre!(
                "File is not a .bmp image, format is not supported: {:?}",
                absolute_path
            ))
        }
    }
}
