pub mod button;
pub mod lcd;
pub mod led;

use crate::button::Button;
use crate::lcd::{Lcd, LcdCommand};
use crate::led::{Led, CONE_LED_COUNT};
use color_eyre::eyre;
use color_eyre::eyre::Context;
use embedded_graphics::pixelcolor::{Rgb565, RgbColor};
use ftdi_embedded_hal::libftd2xx::{Ft4232h, Ftdi, FtdiCommon};
use image::{ImageFormat, Luma};
use orb_rgb::Argb;
use std::sync::{Arc, Mutex};
use std::{env, fs};
use tokio::sync::mpsc;

const CONE_FTDI_DEVICE_COUNT: usize = 8;

#[derive(Debug)]
enum Status {
    Connected,
    Disconnected,
}

/// Cone can be created only if connected to the host over USB.
pub struct Cone {
    connection_status: Arc<Mutex<Status>>,
    lcd: Lcd,
    led_strip: Arc<Mutex<Led>>,
    _button: Button,
}

pub enum ConeEvents {
    ButtonPressed(bool),
}

impl Cone {
    /// Create a new Cone instance.
    pub fn new(event_queue: mpsc::UnboundedSender<ConeEvents>) -> eyre::Result<Self> {
        let connection_status = if ftdi_embedded_hal::libftd2xx::list_devices()?.len()
            != CONE_FTDI_DEVICE_COUNT
        {
            return Err(eyre::eyre!(
                "FTDI device count mismatch: cone not connected?"
            ));
        } else {
            let mut device: Ft4232h = Ftdi::with_index(6)?
                .try_into()
                .wrap_err("Failed to initialize FTDI device")?;
            device.reset().wrap_err("Failed to reset")?;
            Arc::new(Mutex::new(Status::Connected))
        };

        let lcd = Lcd::spawn()?;
        let led_strip = Led::spawn()?;
        let _button = Button::spawn(event_queue.clone(), connection_status.clone())?;

        let cone = Cone {
            connection_status,
            lcd,
            led_strip,
            _button,
        };

        Ok(cone)
    }

    pub fn is_connected(&self) -> bool {
        if let Ok(status) = self.connection_status.lock() {
            matches!(*status, Status::Connected)
        } else {
            false
        }
    }

    /// Update the RGB LEDs by passing the values to the LED strip sender.
    pub fn queue_rgb_leds(
        &mut self,
        pixels: &[Argb; CONE_LED_COUNT],
    ) -> eyre::Result<()> {
        self.led_strip
            .lock()
            .expect("cannot lock LED strip mutex")
            .clone_tx()
            .send(*pixels)
            .wrap_err("Failed to send LED strip values")
    }

    pub fn queue_lcd_fill(&mut self, color: Argb) -> eyre::Result<()> {
        let color = Rgb565::new(color.1, color.2, color.3);
        tracing::debug!("LCD fill color: {:?}", color);
        self.lcd
            .clone_tx()
            .send(LcdCommand::Fill(color))
            .wrap_err("Failed to send")
    }

    /// Update the LCD screen with a QR code.
    /// `qr_str` is encoded as a QR code and sent to the LCD screen.
    pub fn queue_lcd_qr_code(&mut self, qr_str: String) -> eyre::Result<()> {
        let qr_code = qrcode::QrCode::new(qr_str.as_bytes())?
            .render::<Luma<u8>>()
            .dark_color(Luma([0_u8]))
            .light_color(Luma([255_u8]))
            .quiet_zone(true) // disable quiet zone (white border)
            .min_dimensions(200, 200)
            .max_dimensions(230, 230) // sets maximum image size
            .build();
        let mut buffer = std::io::Cursor::new(vec![]);
        qr_code.write_to(&mut buffer, ImageFormat::Bmp)?;
        tracing::debug!("LCD QR: {:?}", qr_str);
        self.lcd
            .clone_tx()
            .send(LcdCommand::ImageBmp(buffer.into_inner(), Rgb565::WHITE))
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
            self.lcd
                .clone_tx()
                .send(LcdCommand::ImageBmp(bmp_data, Rgb565::BLACK))
                .wrap_err("Failed to send")
        } else {
            Err(eyre::eyre!(
                "File is not a .bmp image, format is not supported: {:?}",
                absolute_path
            ))
        }
    }
}

impl Drop for Cone {
    fn drop(&mut self) {
        tracing::debug!("Dropping the Cone");
        // we own an `Arc<Mutex<Led>>` so we need to call `shutdown()` to drop the other
        // reference to the `Arc<Mutex<Led>>`.
        // Otherwise, dropping the `Arc` would simply decrement the reference count and
        // the `Led` would not be dropped.
        self.led_strip.lock().unwrap().shutdown();

        // the rest can be dropped normally
    }
}
