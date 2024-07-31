use color_eyre::eyre;
use color_eyre::eyre::Context;
use ftdi_embedded_hal::eh1::spi::SpiBus;
use ftdi_embedded_hal::libftd2xx::{Ft4232h, Ftdi, FtdiCommon};
use orb_rgb::Argb;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, watch};
use tokio::task;

pub struct Led {
    spi: ftdi_embedded_hal::Spi<Ft4232h>,
    led_strip_tx: mpsc::UnboundedSender<[Argb; CONE_LED_COUNT]>,
    task_handle: Option<task::JoinHandle<eyre::Result<()>>>,
    shutdown_signal: watch::Sender<()>,
}

pub const CONE_LED_COUNT: usize = 64;

impl Led {
    pub(crate) fn spawn() -> eyre::Result<Arc<Mutex<Led>>> {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let (shutdown_signal, mut shutdown_receiver) = watch::channel(());

        let spi = {
            let mut device: Ft4232h = Ftdi::with_index(5)?.try_into()?;
            device.reset().wrap_err("Failed to reset")?;
            let hal = ftdi_embedded_hal::FtHal::init_freq(device, 3_000_000)?;
            hal.spi()?
        };

        let led = Arc::new(Mutex::new(Led {
            spi,
            led_strip_tx: tx.clone(),
            task_handle: None,
            shutdown_signal,
        }));

        // spawn receiver thread
        // where SPI communication happens
        let led_clone = Arc::clone(&led);
        let task = task::spawn(async move {
            loop {
                // todo do we want to update the LED strip at a fixed rate?
                // todo do we want to only take the last message and ignore previous ones
                tokio::select! {
                    _ = shutdown_receiver.changed() => {
                        return Ok(());
                    }
                    msg = rx.recv() => {
                        if let Some(msg) = msg {
                            if let Ok(mut led) = led_clone.lock() {
                                if let Err(e) = led.spi_rgb_led_update_rgb(&msg) {
                                    tracing::debug!("Failed to update LED strip: {e}");
                                }
                            }
                        } else {
                            // none: channel closed
                            return Err(eyre::eyre!("LED strip receiver channel closed"));
                        }
                    }
                }
            }
        });

        if let Ok(led) = &mut led.lock() {
            led.task_handle = Some(task);
        }

        tracing::debug!("LED strip initialized");

        Ok(led)
    }

    pub(crate) fn clone_tx(&self) -> mpsc::UnboundedSender<[Argb; CONE_LED_COUNT]> {
        self.led_strip_tx.clone()
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

    /// Shutdown the LED strip task
    /// This will free at least one Arc<Mutex<Led>> reference
    /// owned by the inner task.
    pub fn shutdown(&self) {
        let _ = self.shutdown_signal.send(());
    }
}

impl Drop for Led {
    fn drop(&mut self) {
        let _ = self.shutdown_signal.send(());
        // wait for task_handle to finish
        if let Some(task_handle) = self.task_handle.take() {
            task::spawn_blocking(move || {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async {
                    let _ = task_handle.await.unwrap();
                    tracing::debug!("LED strip task finished");
                });
            });
        }
    }
}
