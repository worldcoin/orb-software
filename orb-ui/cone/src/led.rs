use color_eyre::eyre;
use ftdi_embedded_hal::eh1::spi::SpiBus;
use ftdi_embedded_hal::libftd2xx::{Ft4232h, Ftdi};
use orb_rgb::Argb;
use tokio::sync::mpsc;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::task;
use tracing::debug;

pub struct Led {
    spi: ftdi_embedded_hal::Spi<Ft4232h>,
}

pub const CONE_LED_COUNT: usize = 64;

async fn handle_rgb_update(
    rx: &mut UnboundedReceiver<[Argb; CONE_LED_COUNT]>,
) -> eyre::Result<()> {
    let device: Ft4232h = Ftdi::with_index(5)?.try_into()?;
    let hal = ftdi_embedded_hal::FtHal::init_freq(device, 3_000_000)?;
    let spi = hal.spi()?;
    let mut led = Led { spi };

    loop {
        // todo do we want to update the LED strip at a fixed rate?
        // todo do we want to only take the last message and ignore previous ones
        if let Some(msg) = rx.recv().await {
            if let Err(e) = led.spi_rgb_led_update_rgb(&msg) {
                debug!("Failed to update LED strip: {:?}", e);
            }
        } else {
            return Err(eyre::eyre!("LED strip receiver channel closed"));
        }
    }
}

impl Led {
    pub(crate) fn spawn() -> eyre::Result<mpsc::UnboundedSender<[Argb; CONE_LED_COUNT]>>
    {
        let (tx, mut rx) = mpsc::unbounded_channel();

        // spawn receiver thread
        // where SPI communication happens
        let _task = task::spawn(async move { handle_rgb_update(&mut rx).await });

        debug!("LED SPI bus initialized");

        Ok(tx)
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

    fn spi_rgb_led_update_rgb(
        &mut self,
        pixels: &[Argb; CONE_LED_COUNT],
    ) -> eyre::Result<()> {
        let mut buffer = vec![0; pixels.len() * 4];
        for (i, pixel) in pixels.iter().enumerate() {
            let prefix = if let Some(dimming) = pixel.0 {
                0xE0 | (dimming & 0x1F)
            } else {
                0xE0 | 0x1F
            };

            // APA102 LED strip uses BGR order
            buffer[i * 4] = prefix;
            buffer[i * 4 + 1] = pixel.3;
            buffer[i * 4 + 2] = pixel.2;
            buffer[i * 4 + 3] = pixel.1;
        }

        self.spi_rgb_led_update(buffer.as_slice())
    }
}
