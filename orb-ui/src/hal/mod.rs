use crate::dbus::OutboundInterfaceProxy;
use crate::engine::TxEvent;
use orb_cone::{Cone, ConeEvents};
use orb_messages::mcu_main::mcu_message::{Message as MainMcuMessage, Message};
use orb_rgb::Argb;
use tokio::sync::mpsc;
use tokio::task;
use tracing::{debug, info};
use zbus::Connection;

pub mod serial;

#[allow(clippy::large_enum_variant)]
pub enum HalMessage {
    Mcu(MainMcuMessage),
    ConeLed([Argb; orb_cone::led::CONE_LED_COUNT]),
    #[allow(dead_code)]
    ConeLcdImage(String),
    #[allow(dead_code)]
    ConeLcdQrCode(String),
}

pub const INPUT_CAPACITY: usize = 100;

/// HAL - Hardware Abstraction Layer
pub struct Hal {
    _thread_to_cone: task::JoinHandle<()>,
    _thread_from_cone: task::JoinHandle<eyre::Result<()>>,
}

impl Hal {
    pub fn spawn(hal_rx: mpsc::Receiver<HalMessage>) -> eyre::Result<Hal> {
        let (cone_tx, mut cone_rx) = mpsc::unbounded_channel();
        let mut cone = Cone::new(cone_tx)?;
        let (serial_tx, serial_rx) = futures::channel::mpsc::channel(INPUT_CAPACITY);
        serial::Serial::spawn(serial_rx)?;

        // send messages to mcu and cone
        let to_hardware = task::spawn(async move {
            handle_hal_update(&mut cone, hal_rx, serial_tx).await
        });

        // handle messages from cone and relay them via dbus
        let from_cone =
            task::spawn(async move { handle_cone_events(&mut cone_rx).await });

        Ok(Hal {
            _thread_to_cone: to_hardware,
            _thread_from_cone: from_cone,
        })
    }
}

async fn handle_hal_update(
    cone: &mut Cone,
    mut hal_rx: mpsc::Receiver<HalMessage>,
    mut serial_tx: futures::channel::mpsc::Sender<Message>,
) {
    loop {
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
                if let Err(s) = cone.queue_rgb_leds(&leds) {
                    tracing::error!("Failed to update LEDs: {:?}", s)
                }
            }
            Some(HalMessage::ConeLcdImage(lcd)) => {
                if let Err(e) = cone.queue_lcd_bmp(lcd) {
                    tracing::error!("Failed to update LCD (bmp image): {:?}", e)
                }
            }
            Some(HalMessage::ConeLcdQrCode(data)) => {
                if let Err(e) = cone.queue_lcd_qr_code(data) {
                    tracing::error!("Failed to update LCD (raw): {:?}", e)
                }
            }

            None => {
                info!("UI event channel closed, stopping cone interface");
                break;
            }
        }
    }
}

async fn handle_cone_events(
    cone_rx: &mut mpsc::UnboundedReceiver<ConeEvents>,
) -> eyre::Result<()> {
    let mut button_pressed = false;
    let connection = Connection::session().await?;
    let proxy = OutboundInterfaceProxy::new(&connection).await?;

    loop {
        match cone_rx.recv().await.expect("cone events channel closed") {
            ConeEvents::ButtonPressed(state) => {
                let tx_event = if state {
                    TxEvent::ConeButtonPressed
                } else {
                    TxEvent::ConeButtonReleased
                };
                if let Err(e) = proxy.ui_event(serde_json::to_string(&tx_event)?).await
                {
                    tracing::warn!("Error: {:#?}", e);
                }
                if state != button_pressed {
                    debug!("🔘 Button {}", if state { "pressed" } else { "released" });
                }
                button_pressed = state;
            }
        }
    }
}
