use color_eyre::eyre;
use ftdi_embedded_hal::eh1::spi::SpiBus;
use ftdi_embedded_hal::libftd2xx::Ft4232h;
use tracing::debug;

/// RGB LED color.
#[derive(Eq, PartialEq, Copy, Clone, Default, Debug)]
pub struct Argb(
    pub Option<u8>, /* optional, dimming value */
    pub u8,
    pub u8,
    pub u8,
);

pub struct Led {
    spi: ftdi_embedded_hal::Spi<Ft4232h>,
}

pub const CONE_LED_COUNT: usize = 64;

impl Led {
    pub(crate) fn new() -> eyre::Result<Self> {
        let device = Ft4232h::with_serial_number("FT80R36LB")?;
        let hal = ftdi_embedded_hal::FtHal::init_freq(device, 3_000_000)?;
        let spi = hal.spi()?;

        debug!("LED SPI bus initialized");

        Ok(Led { spi })
    }

    fn spi_rgb_led_update(&mut self, buffer: &[u8]) -> eyre::Result<()> {
        const ZEROS: [u8; 4] = [0_u8; 4];
        let size = buffer.len();
        let ones_len = (size / 4) / 8 / 2 + 1;
        let ones = vec![0xFF; ones_len];

        // Start frame: at least 32 zeros
        self.spi.write(&ZEROS)?;

        // LED data itself
        self.spi.write(buffer)?;

        // End frame: at least (size / 4) / 2 ones to clock remaining bits
        self.spi.write(ones.as_slice())?;

        Ok(())
    }

    pub fn spi_rgb_led_update_rgb(
        &mut self,
        pixels: &[Argb], // Assuming LedRgb is defined elsewhere
    ) -> eyre::Result<()> {
        let mut buffer = vec![0; pixels.len() * 4];
        for (i, pixel) in pixels.iter().enumerate() {
            let prefix = if let Some(dimming) = pixel.0 {
                0xE0 | (dimming & 0x1F)
            } else {
                0xE0 | 0x1F
            };

            buffer[i * 4] = prefix;
            buffer[i * 4 + 1] = pixel.1;
            buffer[i * 4 + 2] = pixel.2;
            buffer[i * 4 + 3] = pixel.3;
        }

        self.spi_rgb_led_update(buffer.as_slice())
    }
}
