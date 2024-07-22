use crate::{ConeEvents, Status};
use color_eyre::eyre;
use ftdi_embedded_hal::libftd2xx::{BitMode, Ft4232h, Ftdi, FtdiCommon};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tracing::debug;

const BUTTON_GPIO_PIN: u8 = 0;
const BUTTON_GPIO_MASK: u8 = 1 << BUTTON_GPIO_PIN;

pub struct Button {
    thread_handle: Option<std::thread::JoinHandle<()>>,
    terminate: Arc<AtomicBool>,
}

/// Poll the button state.
/// Events are sent to the event queue when the button is pressed or released
/// The thread that polls the button is also used to check the connection status
impl Button {
    pub(crate) fn spawn(
        event_queue: mpsc::UnboundedSender<ConeEvents>,
        connection: Arc<Mutex<Status>>,
    ) -> eyre::Result<Self> {
        let mut device: Ft4232h = Ftdi::with_index(7)?.try_into()?;
        let mask: u8 = !BUTTON_GPIO_MASK; // button pin as input, all others as output
        device.set_bit_mode(mask, BitMode::AsyncBitbang)?;
        debug!("Button GPIO initialized");

        let terminate = Arc::new(AtomicBool::new(false));
        let terminate_clone = Arc::clone(&terminate);

        // spawn a thread to poll the button
        let thread_handle = std::thread::spawn(move || {
            // keep state so that we send an event only on state change
            let mut last_state = false;
            loop {
                if terminate_clone.load(Ordering::Relaxed) {
                    return;
                }
                match device.bit_mode() {
                    Ok(mode) => {
                        // connected
                        if let Ok(mut status) = connection.lock() {
                            *status = Status::Connected;
                        }
                        // button is active low
                        let pressed = mode & BUTTON_GPIO_MASK == 0;
                        if pressed != last_state {
                            if let Err(e) =
                                event_queue.send(ConeEvents::ButtonPressed(pressed))
                            {
                                tracing::error!("Error sending event: {:?} - no receiver? stopping producer", e);
                                return;
                            }
                            last_state = pressed;
                        }
                    }
                    Err(e) => {
                        // disconnected
                        if let Ok(mut status) = connection.lock() {
                            *status = Status::Disconnected;
                        }
                        tracing::trace!("Error reading button state: {:?}", e);
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        });

        Ok(Button {
            thread_handle: Some(thread_handle),
            terminate,
        })
    }
}

impl Drop for Button {
    fn drop(&mut self) {
        self.terminate.store(true, Ordering::Relaxed);
        if let Some(handle) = self.thread_handle.take() {
            handle.join().unwrap();
            debug!("Button thread joined");
        }
    }
}
