pub mod button;
pub mod lcd;
pub mod led;

use crate::button::{Button, ButtonJoinHandle};
use crate::lcd::{Lcd, LcdCommand, LcdJoinHandle};
use crate::led::{LedJoinHandle, LedStrip, CONE_LED_COUNT};
use color_eyre::eyre;
use color_eyre::eyre::Context;
use embedded_graphics::pixelcolor::{Rgb565, RgbColor};
use ftdi_embedded_hal::libftd2xx::{Ft4232h, Ftdi, FtdiCommon};
use image::{ImageFormat, Luma};
use orb_rgb::Argb;
use std::{env, fs};
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
    lcd: LcdJoinHandle,
    led_strip: LedJoinHandle,
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
    lcd: Lcd,
    led_strip: LedStrip,
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

    /// Update the RGB LEDs by passing the values to the LED strip sender.
    pub fn queue_rgb_leds(
        &mut self,
        pixels: &[Argb; CONE_LED_COUNT],
    ) -> eyre::Result<()> {
        self.led_strip
            .send(pixels)
            .wrap_err("Failed to send LED strip values")
    }

    pub fn queue_lcd_fill(&mut self, color: Argb) -> eyre::Result<()> {
        let color = Rgb565::new(color.1, color.2, color.3);
        tracing::debug!("LCD fill color: {:?}", color);
        self.lcd
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
