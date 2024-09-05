use crate::CONE_FTDI_LED_INDEX;
use color_eyre::eyre;
use color_eyre::eyre::{eyre, Context};
use ftdi_embedded_hal::eh1::spi::SpiBus;
use ftdi_embedded_hal::libftd2xx::{Ft4232h, Ftdi, FtdiCommon};
use orb_rgb::Argb;
use tokio::sync::{mpsc, oneshot};
use tokio::task;

pub const CONE_LED_COUNT: usize = 64;

/// LED strip handle.
/// To send new values to the LED strip.
#[derive(Debug)]
pub struct LedStrip {
    /// Used to signal that the task should be cleanly terminated.
    pub kill_tx: oneshot::Sender<()>,
    tx: mpsc::Sender<[Argb; CONE_LED_COUNT]>,
}

#[derive(Debug)]
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
#[derive(Debug)]
struct Apa102<S> {
    spi: S,
}

/// Driver implementation for the APA102 LED strip.
impl<S: SpiBus> Apa102<S> {
    fn spi_rgb_led_update(&mut self, buffer: &[u8]) -> eyre::Result<()> {
        const ZEROS: [u8; 4] = [0_u8; 4];
        let size = buffer.len();
        // The number of clock pulses required is exactly half the total number of LEDs in the string
        let led_count = size / 4;
        let ones_len_bytes = led_count / 8 /* 8 bits per byte */ / 2 + 1 /* at least one stop byte */;
        let ones = vec![0xFF; ones_len_bytes];

        // Start frame: at least 32 zeros
        self.spi
            .write(&ZEROS)
            .map_err(|e| eyre!("err writing: {e:?}"))?;

        // LED data itself
        self.spi
            .write(buffer)
            .map_err(|e| eyre!("err writing: {e:?}"))?;

        // End frame: at least (size / 4) / 2 ones to clock remaining bits
        self.spi
            .write(ones.as_slice())
            .map_err(|e| eyre!("err writing: {e:?}"))?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use ftdi_embedded_hal::eh1::spi::{Error, ErrorKind, ErrorType};
    use std::cell::RefCell;
    use std::fmt::{Debug, Formatter};

    // Mock Spi struct
    struct MockSpi {
        written: RefCell<Vec<u8>>,
    }

    enum MockError {}

    impl Debug for MockError {
        fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
            f.write_str("mocked error")
        }
    }

    impl Error for MockError {
        fn kind(&self) -> ErrorKind {
            ErrorKind::Other
        }
    }

    impl ErrorType for MockSpi {
        type Error = MockError;
    }

    impl SpiBus for MockSpi {
        fn read(&mut self, _words: &mut [u8]) -> Result<(), Self::Error> {
            unimplemented!();
        }

        fn write(&mut self, words: &[u8]) -> Result<(), Self::Error> {
            self.written.borrow_mut().extend_from_slice(words);
            Ok(())
        }

        fn transfer(
            &mut self,
            _read: &mut [u8],
            _write: &[u8],
        ) -> Result<(), Self::Error> {
            unimplemented!();
        }

        fn transfer_in_place(&mut self, _words: &mut [u8]) -> Result<(), Self::Error> {
            unimplemented!();
        }

        fn flush(&mut self) -> Result<(), Self::Error> {
            unimplemented!();
        }
    }

    impl MockSpi {
        fn new() -> Self {
            MockSpi {
                written: RefCell::new(Vec::new()),
            }
        }
    }

    #[test]
    fn test_spi_rgb_led_update() {
        let mock_spi = MockSpi::new();
        let mut apa102 = Apa102 { spi: mock_spi };

        let buffer = [1, 2, 3, 4, 5, 6, 7, 8];
        apa102.spi_rgb_led_update(&buffer).unwrap();

        let written = apa102.spi.written.borrow();

        // Check start frame
        assert_eq!(&written[0..4], &[0, 0, 0, 0]);

        // Check LED data
        assert_eq!(&written[4..12], &buffer);

        // Check end frame (1 byte (8 bits) of 0xFF)
        assert!(written.len() >= 13);
        assert_eq!(&written[12..13], &[0xFF]);
    }

    #[test]
    fn test_spi_rgb_led_update_rgb() {
        let mock_spi = MockSpi::new();
        let mut apa102 = Apa102 { spi: mock_spi };

        let pixels = [Argb(Some(10), 255, 128, 64); CONE_LED_COUNT];
        apa102.spi_rgb_led_update_rgb(&pixels).unwrap();

        let written = apa102.spi.written.borrow();

        // Check start frame
        assert_eq!(&written[0..4], &[0, 0, 0, 0]);

        // Check first LED data
        assert_eq!(&written[4..8], &[0xEA, 64, 128, 255]);

        // Check that we have the correct number of LED data bytes
        // CONE_LED_COUNT=64
        assert_eq!(written.len(), 4 + (CONE_LED_COUNT * 4) + 5);

        // Check end frame (at least 8 bytes of 0xFF)
        assert_eq!(&written[written.len() - 5..], &[0xFF; 5]);
    }
}
