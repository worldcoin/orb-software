use crate::CONE_FTDI_LED_INDEX;
use color_eyre::eyre;
use color_eyre::eyre::Context;
use ftdi_embedded_hal::eh1::spi::SpiBus;
use ftdi_embedded_hal::libftd2xx::{Ft4232h, Ftdi, FtdiCommon};
use orb_rgb::Argb;
use tokio::sync::{mpsc, oneshot};
use tokio::task;

pub const CONE_LED_COUNT: usize = 64;

/// LED strip handle.
/// To send new values to the LED strip.
pub struct LedStrip {
    /// Used to signal that the task should be cleanly terminated.
    pub kill_tx: oneshot::Sender<()>,
    tx: mpsc::Sender<[Argb; CONE_LED_COUNT]>,
}

pub struct LedJoinHandle(pub task::JoinHandle<eyre::Result<()>>);

/// The channel will buffer up to 2 LED frames.
/// If the receiver is full, the frame should be dropped, so that any new frame containing
/// the latest state can be sent once the receiver is ready to receive them.
const LED_CHANNEL_SIZE: usize = 2;

impl LedStrip {
    pub(crate) fn spawn() -> eyre::Result<(Self, LedJoinHandle)> {
        let (tx, mut rx) = mpsc::channel(LED_CHANNEL_SIZE);
        let (kill_tx, mut kill_rx) = oneshot::channel();

        // spawn receiver thread
        // where SPI communication happens
        let task = task::spawn_blocking(move || {
            let spi = {
                let mut device: Ft4232h =
                    Ftdi::with_index(CONE_FTDI_LED_INDEX)?.try_into()?;
                device.reset().wrap_err("Failed to reset")?;
                let hal = ftdi_embedded_hal::FtHal::init_freq(device, 3_000_000)?;
                hal.spi()?
            };

            let mut led = Apa102 { spi };

            let rt = tokio::runtime::Handle::current();
            loop {
                // todo do we want to update the LED strip at a fixed rate?
                // todo do we want to only take the last message and ignore previous ones
                let msg = rt.block_on(async {
                    tokio::select! {
                        _ = &mut kill_rx => {
                            tracing::trace!("led task killed");
                            None
                        }
                        msg = rx.recv() => msg,
                    }
                });

                match msg {
                    Some(values) => {
                        tracing::trace!("led strip values: {:?}", values);
                        if let Err(e) = led.spi_rgb_led_update_rgb(&values) {
                            tracing::debug!("Failed to update LED strip: {e}");
                        } else {
                            tracing::trace!("LED strip updated");
                        }
                    }
                    None => return Ok(()),
                }
            }
        });

        tracing::debug!("LED strip initialized");

        Ok((LedStrip { tx, kill_tx }, LedJoinHandle(task)))
    }

    pub fn tx(&self) -> &mpsc::Sender<[Argb; CONE_LED_COUNT]> {
        &self.tx
    }
}

/// APA102 LEDs
struct Apa102 {
    spi: ftdi_embedded_hal::Spi<Ft4232h>,
}

/// Driver implementation for the APA102 LED strip.
impl Apa102 {
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
