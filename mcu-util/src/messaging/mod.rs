use std::process;
use std::sync::mpsc;

use async_trait::async_trait;
use eyre::{eyre, Result};
use orb_messages::CommonAckError;
use tracing::debug;

pub mod can;
pub mod serial;

#[derive(Clone, Debug)]
pub enum McuPayload {
    ToMain(orb_messages::mcu_main::jetson_to_mcu::Payload),
    ToSec(orb_messages::mcu_sec::jetson_to_sec::Payload),
    FromMain(orb_messages::mcu_main::mcu_to_jetson::Payload),
    FromSec(orb_messages::mcu_sec::sec_to_jetson::Payload),
}

/// CAN(-FD) addressing scheme
#[derive(Clone, Copy, PartialEq, Debug)]
#[allow(dead_code)]
pub enum Device {
    Main = 0x01,
    Security = 0x02,
    JetsonFromMain = 0x80,
    JetsonFromSecurity = 0x81,
}

impl From<u8> for Device {
    fn from(device: u8) -> Device {
        match device {
            0x01 => Device::Main,
            0x02 => Device::Security,
            0x80 => Device::JetsonFromMain,
            0x81 => Device::JetsonFromSecurity,
            _ => panic!("Unknown device: {}", device),
        }
    }
}

#[async_trait]
pub(crate) trait MessagingInterface {
    async fn send(&mut self, payload: McuPayload) -> Result<CommonAckError>;
}

/// Create a unique ack number
/// - prefix with process ID
/// - suffix with counter
/// this added piece of information in the ack number is not strictly necessary
/// but helps filter out acks that are not for us (e.g. acks for other processes)
#[inline]
fn create_ack(counter: u16) -> u32 {
    process::id() << 16 | counter as u32
}

/// Check that ack contains the process ID
#[inline]
pub fn is_ack_for_us(ack_number: u32) -> bool {
    ack_number >> 16 == process::id()
}

/// handle new main mcu message, reference implementation
fn handle_main_mcu_message(
    message: &orb_messages::mcu_main::McuMessage,
    ack_tx: &mpsc::Sender<(CommonAckError, u32)>,
    new_message_queue: &mpsc::Sender<McuPayload>,
) -> Result<()> {
    match message {
        &orb_messages::mcu_main::McuMessage { version, .. }
            if version != orb_messages::mcu_main::Version::Version0 as i32 =>
        {
            return Err(eyre!("unknown message version {:?}", version));
        }
        orb_messages::mcu_main::McuMessage {
            version: _,
            message:
                Some(orb_messages::mcu_main::mcu_message::Message::MMessage(
                    orb_messages::mcu_main::McuToJetson {
                        payload:
                            Some(orb_messages::mcu_main::mcu_to_jetson::Payload::Ack(ack)),
                    },
                )),
        } => {
            if is_ack_for_us(ack.ack_number) {
                ack_tx.send((CommonAckError::from(ack.error), ack.ack_number))?;
            } else {
                debug!("Ignoring ack # 0x{:x?}", ack.ack_number)
            }
        }
        orb_messages::mcu_main::McuMessage {
            version: _,
            message:
                Some(orb_messages::mcu_main::mcu_message::Message::MMessage(
                    orb_messages::mcu_main::McuToJetson { payload: Some(p) },
                )),
        } => {
            new_message_queue.send(McuPayload::FromMain(p.clone()))?;
        }
        _ => {
            if message.message.is_some() {
                return Err(eyre!("incompatible message: {:?}", message));
            } else {
                debug!("Ignoring empty message")
            }
        }
    }
    Ok(())
}

/// handle new security mcu message, reference implementation
fn handle_sec_mcu_message(
    message: &orb_messages::mcu_sec::McuMessage,
    ack_tx: &mpsc::Sender<(CommonAckError, u32)>,
    new_message_queue: &mpsc::Sender<McuPayload>,
) -> Result<()> {
    match message {
        &orb_messages::mcu_sec::McuMessage { version, .. }
            if version != orb_messages::mcu_sec::Version::Version0 as i32 =>
        {
            return Err(eyre!("unknown message version {:?}", version));
        }
        orb_messages::mcu_sec::McuMessage {
            version: _,
            message:
                Some(orb_messages::mcu_sec::mcu_message::Message::SecToJetsonMessage(
                    orb_messages::mcu_sec::SecToJetson {
                        payload:
                            Some(orb_messages::mcu_sec::sec_to_jetson::Payload::Ack(ack)),
                    },
                )),
        } => {
            if is_ack_for_us(ack.ack_number) {
                ack_tx.send((CommonAckError::from(ack.error), ack.ack_number))?;
            }
        }
        orb_messages::mcu_sec::McuMessage {
            version: _,
            message:
                Some(orb_messages::mcu_sec::mcu_message::Message::SecToJetsonMessage(
                    orb_messages::mcu_sec::SecToJetson { payload: Some(p) },
                )),
        } => {
            new_message_queue.send(McuPayload::FromSec(p.clone()))?;
        }
        _ => {
            if message.message.is_some() {
                return Err(eyre!("incompatible message: {:?}", message));
            } else {
                debug!("Ignoring empty message")
            }
        }
    }
    Ok(())
}
