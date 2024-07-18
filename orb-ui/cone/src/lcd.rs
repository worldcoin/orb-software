use color_eyre::eyre;
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

impl<'a> Lcd<'a> {
    pub(crate) fn spawn() -> eyre::Result<mpsc::UnboundedSender<Vec<u8>>> {
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
    rx: &mut mpsc::UnboundedReceiver<Vec<u8>>,
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

    loop {
        if let Some(image) = rx.recv().await {
            // turn back on in case it was turned off
            if let Err(e) = lcd.on() {
                tracing::error!("Error turning on backlight: {:?}", e);
            }
            lcd.display.clear();

            match Bmp::from_slice(image.as_slice()) {
                Ok(bmp) => {
                    // center image
                    let width = bmp.size().width;
                    let height = bmp.size().height;
                    let x = (240 - width as i32) / 2;
                    let y = (240 - height as i32) / 2;
                    let image = Image::new(&bmp, Point::new(x, y));
                    image
                        .draw(&mut lcd.display)
                        .map_err(|e| eyre::eyre!("Error drawing image: {:?}", e))
                        .unwrap();
                }
                Err(e) => {
                    tracing::error!("Error loading image: {:?}", e);
                }
            }
            lcd.display
                .flush()
                .map_err(|e| eyre::eyre!("Error flushing display: {:?}", e))
                .unwrap();
        }
    }
}
