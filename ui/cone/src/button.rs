use crate::{ButtonState, ConeEvents, CONE_FTDI_BUTTON_INDEX};
use color_eyre::eyre;
use ftdi_embedded_hal::libftd2xx::{BitMode, Ft4232h, Ftdi, FtdiCommon};
use std::cmp::PartialEq;
use tokio::select;
use tokio::sync::{broadcast, oneshot};
use tokio::task::JoinHandle;

/// Button connected to pin 0 of the port.
const BUTTON_GPIO_PIN: u8 = 0;
/// Mask to extract the button pin state in the GPIO register (`bit_mode`)
const BUTTON_GPIO_MASK: u8 = 1 << BUTTON_GPIO_PIN;
/// Only the button pin is an input (set to 0), the rest are set to outputs (set to 1)
const BUTTON_GPIO_DIRECTION: u8 = !(1 << BUTTON_GPIO_PIN);
const BUTTON_POLL_INTERVAL_MS: u64 = 50;

/// Handle that can be used to join on errors from the [`Button`] task.
///
/// Note that dropping this handle doesn't kill the task.
#[derive(Debug)]
pub struct ButtonJoinHandle(pub JoinHandle<eyre::Result<()>>);

/// Provides access to the button. Dropping this kills the task.
#[derive(Debug)]
pub struct Button {
    /// Used to signal that the button's task should be cleanly terminated.
    pub kill_tx: oneshot::Sender<()>,
}

impl PartialEq for ButtonState {
    fn eq(&self, other: &Self) -> bool {
        matches!(
            (self, other),
            (ButtonState::Pressed, ButtonState::Pressed)
                | (ButtonState::Released, ButtonState::Released)
        )
    }
}

/// Poll the button state.
/// Events are sent to the event queue when the button is pressed or released
impl Button {
    pub(crate) fn spawn(
        event_queue: broadcast::Sender<ConeEvents>,
    ) -> eyre::Result<(Self, ButtonJoinHandle)> {
        let mut device: Ft4232h =
            Ftdi::with_index(CONE_FTDI_BUTTON_INDEX)?.try_into()?;
        device.set_bit_mode(BUTTON_GPIO_DIRECTION, BitMode::AsyncBitbang)?;
        tracing::debug!("Button GPIO initialized");

        let (kill_tx, mut kill_rx) = oneshot::channel();

        // spawn a thread to poll the button
        let thread_handle = tokio::task::spawn_blocking(move || {
            // keep state so that we send an event only on state change
            let mut last_state = ButtonState::Released;
            let rt = tokio::runtime::Handle::current();
            loop {
                let interval = rt.block_on(async {
                    select! {
                    _ = &mut kill_rx => None,
                    _ = tokio::time::sleep(std::time::Duration::from_millis(BUTTON_POLL_INTERVAL_MS)) => Some(()) }
                });

                match interval {
                    Some(_) => {
                        match device.bit_mode() {
                            Ok(mode) => {
                                // button is active low
                                let state = if mode & BUTTON_GPIO_MASK == 0 {
                                    ButtonState::Pressed
                                } else {
                                    ButtonState::Released
                                };

                                if state != last_state {
                                    if let Err(e) =
                                        event_queue.send(ConeEvents::Button(state))
                                    {
                                        tracing::debug!("Error sending event: {e:?} - no receiver? stopping producer");
                                        return Ok(());
                                    }
                                    last_state = state;
                                }
                            }
                            Err(e) => {
                                tracing::trace!("bit_mode() returned: {:?}", e);
                                return Err(eyre::eyre!(
                                    "Error reading button state: {e:?}"
                                ));
                            }
                        }
                    }
                    None => return Ok(()),
                }
            }
        });

        Ok((Button { kill_tx }, ButtonJoinHandle(thread_handle)))
    }
}
