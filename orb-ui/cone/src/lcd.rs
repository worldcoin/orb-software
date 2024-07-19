use color_eyre::eyre;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::primitives::{PrimitiveStyleBuilder, Rectangle};
use embedded_graphics::{image::Image, prelude::*};
use ftdi_embedded_hal::eh1::digital::OutputPin;
use ftdi_embedded_hal::libftd2xx::{Ft4232h, Ftdi};
use ftdi_embedded_hal::{Delay, SpiDevice};
use gc9a01::{mode::BufferedGraphics, prelude::*, Gc9a01, SPIDisplayInterface};
use tinybmp::Bmp;
use tokio::sync::mpsc;
use tokio::task;
use tracing::debug;

type LcdDisplayDriver<'a> = Gc9a01<
    SPIInterface<&'a SpiDevice<Ft4232h>, ftdi_embedded_hal::OutputPin<Ft4232h>>,
    DisplayResolution240x240,
    BufferedGraphics<DisplayResolution240x240>,
>;

#[allow(dead_code)]
pub struct Lcd<'a> {
    display: LcdDisplayDriver<'a>,
    pub bl: ftdi_embedded_hal::OutputPin<Ft4232h>,
}

/// Commands to the LCD
pub enum LcdCommand {
    /// Display a BMP image on the LCD with a background color, image is centered on the screen
    ImageBmp(Vec<u8>, Rgb565),
    /// Fill the LCD with a color
    Fill(Rgb565),
}

impl<'a> Lcd<'a> {
    pub(crate) fn spawn() -> eyre::Result<mpsc::UnboundedSender<LcdCommand>> {
        let (tx, mut rx) = mpsc::unbounded_channel();

        task::spawn(async move {
            handle_lcd_update(&mut rx).await.map_err(|e| {
                tracing::error!("Error handling LCD update: {:?}", e);
            })
        });

        Ok(tx)
    }

    pub fn on(&mut self) -> eyre::Result<()> {
        self.bl
            .set_high()
            .map_err(|e| eyre::eyre!("Error setting backlight high: {:?}", e))?;
        Ok(())
    }

    pub fn off(&mut self) -> eyre::Result<()> {
        self.bl
            .set_low()
            .map_err(|e| eyre::eyre!("Error setting backlight low: {:?}", e))?;
        Ok(())
    }
}

async fn handle_lcd_update(
    rx: &mut mpsc::UnboundedReceiver<LcdCommand>,
) -> eyre::Result<()> {
    let mut delay = Delay::new();
    let device: Ft4232h = Ftdi::with_index(4)?.try_into()?;
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

    let mut lcd = Lcd { display, bl };

    debug!("LCD SPI bus initialized");

    // from there, never return
    loop {
        if let Some(image) = rx.recv().await {
            // turn back on in case it was turned off
            if let Err(e) = lcd.on() {
                tracing::error!("Error turning on backlight: {:?}", e);
            }
            lcd.display.clear();

            match image {
                LcdCommand::ImageBmp(image, bg_color) => {
                    match Bmp::from_slice(image.as_slice()) {
                        Ok(bmp) => {
                            // draw background color
                            if let Err(e) = fill_color(&mut lcd.display, bg_color) {
                                tracing::error!("{e:?}");
                            }

                            // compute center position for image
                            let width = bmp.size().width as i32;
                            let height = bmp.size().height as i32;
                            let x =
                                (DisplayResolution240x240::WIDTH as i32 - width) / 2;
                            let y =
                                (DisplayResolution240x240::HEIGHT as i32 - height) / 2;

                            // draw image
                            let image = Image::new(&bmp, Point::new(x, y));
                            if let Err(e) = image.draw(&mut lcd.display) {
                                tracing::error!("{e:?}");
                            }
                        }
                        Err(e) => {
                            tracing::error!("Error loading image: {:?}", e);
                        }
                    }
                }
                LcdCommand::Fill(color) => {
                    if let Err(e) = fill_color(&mut lcd.display, color) {
                        tracing::error!("{e:?}");
                    }
                }
            }

            if let Err(e) = lcd
                .display
                .flush()
                .map_err(|e| eyre::eyre!("Error flushing: {e:?}"))
            {
                tracing::error!("{e:?}");
            }
        }
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
