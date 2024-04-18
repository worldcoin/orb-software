use color_eyre::eyre;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::primitives::{Circle, PrimitiveStyleBuilder, Rectangle};
use embedded_graphics::{image::Image, prelude::*};
use ftdi_embedded_hal::eh0::blocking::delay::DelayMs;
use ftdi_embedded_hal::eh1::digital::OutputPin;
use ftdi_embedded_hal::libftd2xx::Ft4232h;
use ftdi_embedded_hal::Spi;
use gc9a01::{mode::BufferedGraphics, prelude::*, Gc9a01, SPIDisplayInterface};
use tinybmp::Bmp;
use tracing::debug;

type LcdDisplayDriver = Gc9a01<
    SPIInterface<
        Spi<Ft4232h>,
        ftdi_embedded_hal::OutputPin<Ft4232h>,
        ftdi_embedded_hal::OutputPin<Ft4232h>,
    >,
    DisplayResolution240x240,
    BufferedGraphics<DisplayResolution240x240>,
>;

#[allow(dead_code)]
pub struct Lcd {
    display: LcdDisplayDriver,
    pub bl: ftdi_embedded_hal::OutputPin<Ft4232h>,
}

struct Delay {}
impl DelayMs<u8> for Delay {
    fn delay_ms(&mut self, ms: u8) {
        std::thread::sleep(std::time::Duration::from_millis(ms.into()))
    }
}

impl Lcd {
    pub(crate) fn new() -> eyre::Result<Self> {
        let mut delay = Delay {};
        let device = Ft4232h::with_serial_number("FT80R36LA")?;
        let hal = ftdi_embedded_hal::FtHal::init_freq(device, 30_000_000)?;
        let spi = hal.spi()?;
        let cs = hal.ad3()?;
        let mut rst = hal.ad4()?;
        let mut bl = hal.ad5()?;
        let dc = hal.ad6()?;

        bl.set_low()
            .map_err(|e| eyre::eyre!("Error setting backlight low: {:?}", e))?;

        let interface = SPIDisplayInterface::new(spi, dc, cs);
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

        debug!("LCD SPI bus initialized");

        Ok(Lcd { display, bl })
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

    pub fn test(&mut self) -> eyre::Result<()> {
        self.on()?;
        self.display.clear();
        draw(&mut self.display);
        self.display
            .flush()
            .map_err(|e| eyre::eyre!("Error flushing display: {:?}", e))
    }

    pub fn load_bmp(&mut self, image: &Bmp<Rgb565>) -> eyre::Result<()> {
        self.on()?;
        self.display.clear();
        Image::new(image, Point::zero())
            .draw(&mut self.display)
            .map_err(|e| eyre::eyre!("Error drawing image: {:?}", e))?;
        self.display
            .flush()
            .map_err(|e| eyre::eyre!("Error flushing display: {:?}", e))?;

        debug!("LCD image loaded");

        Ok(())
    }
}

/// Test Function : will be removed later
fn draw<I: WriteOnlyDataCommand, D: DisplayDefinition>(
    display: &mut Gc9a01<I, D, BufferedGraphics<D>>,
) {
    let style = PrimitiveStyleBuilder::new()
        .stroke_width(4)
        .stroke_color(Rgb565::new(100, 100, 100))
        .fill_color(Rgb565::RED)
        .build();

    let cdiameter = 20;

    // circle
    Circle::new(
        Point::new(119 - cdiameter / 2 + 40, 119 - cdiameter / 2 + 40),
        cdiameter as u32,
    )
    .into_styled(style)
    .draw(display)
    .unwrap();

    // circle
    Circle::new(
        Point::new(119 - cdiameter / 2 - 40, 119 - cdiameter / 2 + 40),
        cdiameter as u32,
    )
    .into_styled(style)
    .draw(display)
    .unwrap();

    // rectangle
    let rw = 80;
    let rh = 20;
    Rectangle::new(
        Point::new(119 - rw / 2, 119 - rh / 2 - 40),
        Size::new(rw as u32, rh as u32),
    )
    .into_styled(style)
    .draw(display)
    .unwrap();
}
