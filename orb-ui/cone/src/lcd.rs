use color_eyre::eyre;
use color_eyre::eyre::Context;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::primitives::{PrimitiveStyleBuilder, Rectangle};
use embedded_graphics::{image::Image, prelude::*};
use ftdi_embedded_hal::eh1::digital::OutputPin;
use ftdi_embedded_hal::libftd2xx::{Ft4232h, Ftdi, FtdiCommon};
use ftdi_embedded_hal::{Delay, SpiDevice};
use gc9a01::{mode::BufferedGraphics, prelude::*, Gc9a01, SPIDisplayInterface};
use tinybmp::Bmp;
use tokio::sync::watch::Receiver;
use tokio::sync::{mpsc, watch};
use tokio::task;
use tokio::task::JoinHandle;
use tracing::debug;

type LcdDisplayDriver<'a> = Gc9a01<
    SPIInterface<&'a SpiDevice<Ft4232h>, ftdi_embedded_hal::OutputPin<Ft4232h>>,
    DisplayResolution240x240,
    BufferedGraphics<DisplayResolution240x240>,
>;

/// Lcd handle to send commands to the LCD screen.
///
/// The LCD is controlled by a separate task.
/// The task is spawned when the Lcd is created
/// and stopped when the Lcd is dropped
pub struct Lcd {
    tx: mpsc::UnboundedSender<LcdCommand>,
    shutdown_signal: watch::Sender<()>,
    task_handle: Option<JoinHandle<eyre::Result<()>>>,
}

/// Commands to the LCD
pub enum LcdCommand {
    /// Display a BMP image on the LCD with a background color, image is centered on the screen
    ImageBmp(Vec<u8>, Rgb565),
    /// Fill the LCD with a color
    Fill(Rgb565),
}

impl Lcd {
    pub(crate) fn spawn() -> eyre::Result<Lcd> {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let (shutdown_signal, shutdown_receiver) = watch::channel(());

        let task_handle =
            task::spawn(
                async move { handle_lcd_update(&mut rx, shutdown_receiver).await },
            );

        Ok(Lcd {
            tx,
            shutdown_signal,
            task_handle: Some(task_handle),
        })
    }

    pub(crate) fn clone_tx(&self) -> mpsc::UnboundedSender<LcdCommand> {
        self.tx.clone()
    }
}

async fn handle_lcd_update(
    rx: &mut mpsc::UnboundedReceiver<LcdCommand>,
    mut shutdown_receiver: Receiver<()>,
) -> eyre::Result<()> {
    let mut delay = Delay::new();
    let mut device: Ft4232h = Ftdi::with_index(4)?.try_into()?;
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

    loop {
        tokio::select! {
            _ = shutdown_receiver.changed() => {
                debug!("LCD task shutting down");
                return Ok(())
            }
            command = rx.recv() => {
                // turn back on in case it was turned off
                if let Err(e) = bl
                    .set_high() {
                    tracing::info!("Backlight: {e:?}");
                }
                display.clear();

                match command {
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
                                    tracing::info!("{e:?}");
                                }
                            }
                            Err(e) => {
                                tracing::info!("Error loading image: {e:?}");
                            }
                        }
                    }
                    Some(LcdCommand::Fill(color)) => {
                        if let Err(e) = fill_color(&mut display, color) {
                            tracing::info!("{e:?}");
                        }
                    }
                    None => {
                        tracing::info!("LCD channel closed");
                        return Err(eyre::eyre!("LCD channel closed"));
                    }
                }

                if let Err(e) = display
                    .flush()
                    .map_err(|e| eyre::eyre!("Error flushing: {e:?}")) {
                    tracing::info!("{e}");
                }
            }
        }
    }
}

impl Drop for Lcd {
    fn drop(&mut self) {
        let _ = self.shutdown_signal.send(());
        // wait for task_handle to finish
        if let Some(task_handle) = self.task_handle.take() {
            task::spawn_blocking(move || {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async {
                    let _ = task_handle.await.unwrap();
                    debug!("LCD task finished");
                });
            });
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
