use crate::engine::Event;
use futures::channel::mpsc;
use orb_cone::{Cone, ConeEvents, ConeLcd, ConeLeds};
use orb_messages::mcu_main::mcu_message::Message as MainMcuMessage;
use tokio::sync::mpsc::UnboundedSender;
use tracing::{debug, info};

pub mod serial;

#[allow(clippy::large_enum_variant)]
pub enum HalMessage {
    Mcu(MainMcuMessage),
    ConeLed(ConeLeds),
    #[allow(dead_code)] // fixme
    ConeLcd(ConeLcd),
}

pub const INPUT_CAPACITY: usize = 100;

/// HAL - Hardware Abstraction Layer
pub struct Hal {
    _thread_to_cone: std::thread::JoinHandle<()>,
    _thread_from_cone: std::thread::JoinHandle<()>,
}

impl Hal {
    pub fn spawn(
        ui_event_tx: UnboundedSender<Event>,
        mut hal_rx: mpsc::Receiver<HalMessage>,
    ) -> eyre::Result<Hal> {
        let (cone_tx, cone_rx) = std::sync::mpsc::channel();
        let mut cone = Cone::new(cone_tx)?;
        let (mut serial_tx, serial_rx) = mpsc::channel(INPUT_CAPACITY);
        serial::Serial::spawn(serial_rx)?;

        // send messages to mcu and cone
        let to_hardware = std::thread::spawn(move || loop {
            match hal_rx.try_next() {
                Ok(Some(HalMessage::Mcu(m))) => {
                    if let Err(e) = serial_tx.try_send(m) {
                        tracing::error!(
                            "Failed to send message to serial interface: {:?}",
                            e
                        );
                    }
                }
                Ok(Some(HalMessage::ConeLed(leds))) => {
                    if let Err(s) = cone.queue_rgb_leds(&leds) {
                        tracing::error!("Failed to update LEDs: {:?}", s)
                    }
                }
                Ok(Some(HalMessage::ConeLcd(lcd))) => {
                    if let Err(e) = cone.lcd_load_image(lcd.0.as_str()) {
                        tracing::error!("Failed to update LCD: {:?}", e)
                    }
                }
                Ok(None) => {
                    info!("UI event channel closed, stopping cone interface");
                    break;
                }
                Err(_) => {}
            }
        });

        // handle messages from cone
        let from_cone = std::thread::spawn(move || {
            let mut button_pressed = false;
            loop {
                match cone_rx.recv() {
                    Ok(event) => match event {
                        ConeEvents::ButtonPressed(state) => {
                            if button_pressed && !state {
                                if let Err(e) = ui_event_tx.send(Event::SignupStart) {
                                    tracing::error!(
                                        "Failed to send event to engine: {:?}",
                                        e
                                    );
                                }
                            }
                            if state != button_pressed {
                                debug!(
                                    "🔘 Button {}",
                                    if state { "pressed" } else { "released" }
                                );
                            }
                            button_pressed = state;
                        }
                    },
                    Err(e) => {
                        tracing::error!("Error receiving event: {:?}", e);
                        break;
                    }
                }
            }
        });

        Ok(Hal {
            _thread_to_cone: to_hardware,
            _thread_from_cone: from_cone,
        })
    }
}
