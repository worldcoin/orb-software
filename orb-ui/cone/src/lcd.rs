use crate::CONE_FTDI_LCD_INDEX;
use color_eyre::eyre;
use color_eyre::eyre::Context;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::primitives::{PrimitiveStyleBuilder, Rectangle};
use embedded_graphics::{image::Image, prelude::*};
use ftdi_embedded_hal::eh1::digital::OutputPin;
use ftdi_embedded_hal::libftd2xx::{Ft4232h, Ftdi, FtdiCommon};
use ftdi_embedded_hal::{Delay, SpiDevice};
use gc9a01::{mode::BufferedGraphics, prelude::*, Gc9a01, SPIDisplayInterface};
use image::{ImageFormat, Luma};
use orb_rgb::Argb;
use std::fs;
use std::path::Path;
use thiserror::Error;
use tinybmp::Bmp;
use tokio::sync::{mpsc, oneshot};
use tokio::task;
use tokio::task::JoinHandle;

type LcdDisplayDriver<'a> = Gc9a01<
    SPIInterface<&'a SpiDevice<Ft4232h>, ftdi_embedded_hal::OutputPin<Ft4232h>>,
    DisplayResolution240x240,
    BufferedGraphics<DisplayResolution240x240>,
>;

#[derive(Debug)]
pub struct LcdJoinHandle(pub JoinHandle<eyre::Result<()>>);

/// LcdCommand channel size
/// At least a second should be spent between commands for the user
/// to actually see the changes on the screen so the limit should
/// never be a blocker.
const LCD_COMMAND_CHANNEL_SIZE: usize = 2;

/// Lcd handle to send commands to the LCD screen.
///
/// The LCD is controlled by a separate task.
/// The task is spawned when the Lcd is created
/// and stopped when the Lcd is dropped
#[derive(Debug)]
pub struct Lcd {
    /// Used to signal that the task should be cleanly terminated.
    pub kill_tx: oneshot::Sender<()>,
    /// Send commands to the LCD task
    cmd_tx: mpsc::Sender<LcdCommand>,
}

/// Commands to the LCD
#[derive(Debug, Eq, PartialEq, Clone)]
pub enum LcdCommand {
    /// Display a BMP image on the LCD with a background color, image is centered on the screen
    ImageBmp(Vec<u8>, Rgb565),
    /// Fill the LCD with a color
    Fill(Rgb565),
}

#[derive(Error, Debug)]
pub enum LcdCommandError {
    #[error("file not found")]
    FileNotFound,
    #[error("unsupported format")]
    UnsupportedFormat,
    #[error("error reading or writing into file")]
    Io,
}

impl TryFrom<Argb> for LcdCommand {
    type Error = LcdCommandError;

    fn try_from(color: Argb) -> Result<Self, Self::Error> {
        let color = Rgb565::new(color.1, color.2, color.3);
        Ok(LcdCommand::Fill(color))
    }
}

impl TryFrom<&Path> for LcdCommand {
    type Error = LcdCommandError;

    fn try_from(path: &Path) -> Result<Self, Self::Error> {
        // check if file exists, use absolute path for better understanding of the error
        if !path.exists() {
            return Err(LcdCommandError::FileNotFound);
        }

        // check if file is a bmp image
        if path.extension().ok_or(LcdCommandError::UnsupportedFormat)? == "bmp" {
            tracing::debug!("LCD image: {:?}", path);
            let bmp_data = fs::read(path).map_err(|_| LcdCommandError::Io)?;
            Ok(LcdCommand::ImageBmp(bmp_data, Rgb565::BLACK))
        } else {
            Err(LcdCommandError::UnsupportedFormat)
        }
    }
}

impl TryFrom<String> for LcdCommand {
    type Error = LcdCommandError;

    fn try_from(qr_code: String) -> Result<Self, Self::Error> {
        let qr_code = qrcode::QrCode::new(qr_code.as_bytes())
            .map_err(|_| LcdCommandError::UnsupportedFormat)?
            .render::<Luma<u8>>()
            .dark_color(Luma([0_u8]))
            .light_color(Luma([255_u8]))
            .quiet_zone(true) // disable quiet zone (white border)
            .min_dimensions(200, 200)
            .max_dimensions(230, 230) // sets maximum image size
            .build();
        let mut buffer = std::io::Cursor::new(vec![]);
        qr_code
            .write_to(&mut buffer, ImageFormat::Bmp)
            .map_err(|_| LcdCommandError::Io)?;
        tracing::debug!("LCD QR: {:?}", qr_code);
        Ok(LcdCommand::ImageBmp(buffer.into_inner(), Rgb565::WHITE))
    }
}

impl Lcd {
    pub(crate) fn spawn() -> eyre::Result<(Lcd, LcdJoinHandle)> {
        let (cmd_tx, mut cmd_rx) = mpsc::channel(LCD_COMMAND_CHANNEL_SIZE);
        let (kill_tx, kill_rx) = oneshot::channel();

        let task_handle =
            task::spawn_blocking(move || do_lcd_update(&mut cmd_rx, kill_rx));

        Ok((Lcd { cmd_tx, kill_tx }, LcdJoinHandle(task_handle)))
    }

    pub fn tx(&self) -> &mpsc::Sender<LcdCommand> {
        &self.cmd_tx
    }
}

/// Entry point for the lcd update task
fn do_lcd_update(
    cmd_rx: &mut mpsc::Receiver<LcdCommand>,
    mut kill_rx: oneshot::Receiver<()>,
) -> eyre::Result<()> {
    let mut delay = Delay::new();
    let mut device: Ft4232h = Ftdi::with_index(CONE_FTDI_LCD_INDEX)?.try_into()?;
    device.reset().wrap_err("Failed to reset")?;
    let hal = ftdi_embedded_hal::FtHal::init_freq(device, 30_000_000)?;
    let spi = Box::pin(hal.spi_device(3)?);
    let mut rst = hal.ad4()?;
    let mut bl = hal.ad5()?;
    let dc = hal.ad6()?;

    bl.set_low()
        .map_err(|e| eyre::eyre!("Error setting backlight low: {:?}", e))?;

    let interface = SPIDisplayInterface::new(spi.as_ref().get_ref(), dc);
    let mut display = Gc9a01::new(
        interface,
        DisplayResolution240x240,
        DisplayRotation::Rotate180,
    )
    .into_buffered_graphics();
    display
        .reset(&mut rst, &mut delay)
        .map_err(|e| eyre::eyre!("Error resetting display: {:?}", e))?;
    display
        .init(&mut delay)
        .map_err(|e| eyre::eyre!("Error initializing display: {:?}", e))?;
    display.fill(0x0000);
    display
        .flush()
        .map_err(|e| eyre::eyre!("Error flushing display: {:?}", e))?;

    let rt = tokio::runtime::Handle::current();
    loop {
        let cmd = rt.block_on(async {
            tokio::select! {
                _ = &mut kill_rx => None,
                cmd = cmd_rx.recv() => cmd,
            }
        });

        // turn back on in case it was turned off
        bl.set_high()?;
        display.clear();

        match cmd {
            Some(LcdCommand::ImageBmp(image, bg_color)) => {
                match Bmp::from_slice(image.as_slice()) {
                    Ok(bmp) => {
                        // draw background color
                        if let Err(e) = fill_color(&mut display, bg_color) {
                            tracing::info!("{e:?}");
                        }

                        // compute center position for image
                        let width = bmp.size().width as i32;
                        let height = bmp.size().height as i32;
                        let x = (DisplayResolution240x240::WIDTH as i32 - width) / 2;
                        let y = (DisplayResolution240x240::HEIGHT as i32 - height) / 2;

                        // draw image
                        let image = Image::new(&bmp, Point::new(x, y));
                        if let Err(e) = image.draw(&mut display) {
                            tracing::warn!("{e:?}");
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Error loading image: {e:?}");
                    }
                }
            }
            Some(LcdCommand::Fill(color)) => {
                if let Err(e) = fill_color(&mut display, color) {
                    tracing::warn!("{e:?}");
                }
            }
            None => {
                // cmd channel closed or kill_rx received
                let _ = bl.set_low();
                return Ok(());
            }
        }

        display
            .flush()
            .map_err(|e| eyre::eyre!("Error flushing: {e:?}"))?;
    }
}

fn fill_color(display: &mut LcdDisplayDriver, color: Rgb565) -> eyre::Result<()> {
    Rectangle::new(
        Point::new(0, 0),
        Size::new(
            DisplayResolution240x240::WIDTH as u32,
            DisplayResolution240x240::HEIGHT as u32,
        ),
    )
    .into_styled(PrimitiveStyleBuilder::new().fill_color(color).build())
    .draw(display)
    .map_err(|e| eyre::eyre!("Error drawing the rectangle: {e:?}"))
}
