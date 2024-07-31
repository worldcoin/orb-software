use crate::dbus::OutboundInterfaceProxy;
use crate::engine::TxEvent;
use orb_cone::{ButtonState, Cone, ConeEvents};
use orb_messages::mcu_main::mcu_message::{Message as MainMcuMessage, Message};
use orb_rgb::Argb;
use tokio::sync::{broadcast, mpsc};
use tokio::task;
use tracing::{debug, info};
use zbus::Connection;

pub mod serial;

const CONE_RECONNECT_INTERVAL_SECONDS: u64 = 10;

#[allow(clippy::large_enum_variant)]
pub enum HalMessage {
    Mcu(MainMcuMessage),
    ConeLed([Argb; orb_cone::led::CONE_LED_COUNT]),
    ConeLcdQrCode(String),
    ConeLcdFillColor(Argb),
    #[allow(dead_code)]
    ConeLcdImage(String),
}

pub const INPUT_CAPACITY: usize = 100;
pub const CONE_EVENTS_CAPACITY: usize = 20;

/// HAL - Hardware Abstraction Layer
pub struct Hal {
    _thread_to_cone: task::JoinHandle<()>,
    _thread_from_cone: task::JoinHandle<eyre::Result<()>>,
}

impl Hal {
    pub fn spawn(
        hal_rx: mpsc::Receiver<HalMessage>,
        has_cone: bool,
    ) -> eyre::Result<Hal> {
        let (cone_tx, mut cone_rx) = broadcast::channel(CONE_EVENTS_CAPACITY);
        let (serial_tx, serial_rx) = futures::channel::mpsc::channel(INPUT_CAPACITY);
        serial::Serial::spawn(serial_rx)?;

        // send messages to mcu and cone
        let to_hardware = task::spawn(async move {
            handle_hal(
                if has_cone {
                    Some(cone_tx.clone())
                } else {
                    None
                },
                hal_rx,
                serial_tx,
            )
            .await
        });

        // handle messages from cone and relay them via dbus
        let from_cone = task::spawn(async move {
            if let Err(e) = handle_cone_events(&mut cone_rx).await {
                tracing::error!("Error handling cone events: {:?}", e);
                Err(e)
            } else {
                Ok(())
            }
        });

        Ok(Hal {
            _thread_to_cone: to_hardware,
            _thread_from_cone: from_cone,
        })
    }
}

/// Handle messages from the HAL and send them to the appropriate hardware
/// interface.
/// This function is responsible for managing the connection to the cone.
/// If the connection is lost, it will try to reconnect every 10 seconds.
async fn handle_hal(
    cone_tx: Option<broadcast::Sender<ConeEvents>>,
    mut hal_rx: mpsc::Receiver<HalMessage>,
    mut serial_tx: futures::channel::mpsc::Sender<Message>,
) {
    let mut cone_handles = None;

    if let Some(tx) = &cone_tx {
        cone_handles = Cone::spawn(tx.clone()).map_err(|e| eyre::eyre!(e)).ok();
    }

    let mut reconnect_cone_time = std::time::Instant::now();
    loop {
        // try to create a cone if it doesn't exist every 10 seconds
        if cone_handles.is_none()
            && reconnect_cone_time.elapsed().as_secs() > CONE_RECONNECT_INTERVAL_SECONDS
        {
            reconnect_cone_time = std::time::Instant::now();
            if let Some(tx) = &cone_tx {
                cone_handles =
                    Cone::spawn(tx.clone()).map_err(|e| info!("{:}", e)).ok();
                if cone_handles.is_some() {
                    info!("Cone connected");
                }
            }
        } else if cone_handles.is_some() {
            if let Some((c, _)) = &mut cone_handles {
                if !c.is_connected() {
                    info!("Cone disconnected");
                    drop(cone_handles);
                    cone_handles = None;
                }
            }
            reconnect_cone_time = std::time::Instant::now();
        }

        match hal_rx.recv().await {
            Some(HalMessage::Mcu(m)) => {
                if let Err(e) = serial_tx.try_send(m) {
                    tracing::error!(
                        "Failed to send message to serial interface: {:?}",
                        e
                    );
                }
            }
            Some(HalMessage::ConeLed(leds)) => {
                if let Some((cone, _)) = &mut cone_handles {
                    if let Err(s) = cone.queue_rgb_leds(&leds).await {
                        tracing::error!("Failed to update LEDs: {:?}", s)
                    }
                }
            }
            Some(HalMessage::ConeLcdImage(lcd)) => {
                if let Some((cone, _)) = &mut cone_handles {
                    if let Err(e) = cone.queue_lcd_bmp(lcd).await {
                        tracing::error!("Failed to update LCD (bmp image): {:?}", e)
                    }
                }
            }
            Some(HalMessage::ConeLcdQrCode(data)) => {
                if let Some((cone, _)) = &mut cone_handles {
                    if let Err(e) = cone.queue_lcd_qr_code(data).await {
                        tracing::error!("Failed to update LCD (raw): {:?}", e)
                    }
                }
            }
            Some(HalMessage::ConeLcdFillColor(color)) => {
                if let Some((cone, _)) = &mut cone_handles {
                    if let Err(e) = cone.queue_lcd_fill(color).await {
                        tracing::error!("Failed to update LCD (fill): {:?}", e)
                    }
                }
            }
            None => {
                info!("UI event channel closed, stopping cone interface");
                break;
            }
        }
    }

    // wait for all tasks to stop
    if let Some((c, handles)) = cone_handles {
        drop(c);
        let _ = handles.join().await;
    }
}

async fn handle_cone_events(
    cone_rx: &mut broadcast::Receiver<ConeEvents>,
) -> eyre::Result<()> {
    let mut button_pressed_state = ButtonState::Released;
    let connection = Connection::session().await?;
    let proxy = OutboundInterfaceProxy::new(&connection).await?;

    loop {
        match cone_rx.recv().await {
            Ok(ConeEvents::Button(state)) => {
                let tx_event = match state {
                    ButtonState::Pressed => TxEvent::ConeButtonPressed,
                    ButtonState::Released => TxEvent::ConeButtonReleased,
                };

                if let Err(e) =
                    proxy.user_event(serde_json::to_string(&tx_event)?).await
                {
                    tracing::warn!("Error: {:#?}", e);
                }
                if state != button_pressed_state {
                    debug!("🔘 Button {:?}", state);
                }
                button_pressed_state = state;
            }
            Ok(ConeEvents::Cone(state)) => {
                info!("🔌 Cone {:?}", state);
            }
            Err(e) => return Err(eyre::eyre!("Cone event channel closed {e}")),
        }
    }
}
