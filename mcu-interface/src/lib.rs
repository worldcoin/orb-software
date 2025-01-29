use std::process;

use async_trait::async_trait;
use color_eyre::eyre::{eyre, Result};
use orb_messages::CommonAckError;
use tokio::sync::mpsc;
use tracing::debug;

pub mod can;
pub mod serial;

pub use orb_messages;

#[derive(Clone, Debug)]
pub enum McuPayload {
    ToMain(orb_messages::main::jetson_to_mcu::Payload),
    ToSec(orb_messages::sec::jetson_to_sec::Payload),
    FromMain(orb_messages::main::mcu_to_jetson::Payload),
    FromSec(orb_messages::sec::sec_to_jetson::Payload),
}

/// CAN(-FD) addressing scheme
#[derive(Clone, Copy, PartialEq, Debug)]
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
pub trait MessagingInterface {
    async fn send(&mut self, payload: McuPayload) -> Result<CommonAckError>;
}

/// Create a unique ack number
/// - prefix with process ID (16 bits, the least significant bits)
/// - suffix with counter
///
/// this added piece of information in the ack number is not strictly necessary
/// but helps filter out acks that are not for us (e.g. acks for other processes)
#[inline]
fn create_ack(counter: u16) -> u32 {
    (process::id() & 0xFFFF) << 16_u32 | counter as u32
}

/// Check that ack contains 16 least significant bits of the process ID
#[inline]
fn is_ack_for_us(ack_number: u32) -> bool {
    // cast looses the upper bits
    ((ack_number >> 16) as u16) == (process::id() as u16)
}

/// handle new main mcu message, reference implementation
fn handle_main_mcu_message(
    message: &orb_messages::McuMessage,
    ack_tx: &mpsc::UnboundedSender<(CommonAckError, u32)>,
    new_message_queue: &mpsc::UnboundedSender<McuPayload>,
) -> Result<()> {
    match message {
        &orb_messages::McuMessage { version, .. }
            if version != orb_messages::Version::Version0 as i32 =>
        {
            return Err(eyre!("unknown message version {:?}", version));
        }
        orb_messages::McuMessage {
            version: _,
            message:
                Some(orb_messages::mcu_message::Message::MMessage(
                    orb_messages::main::McuToJetson {
                        payload:
                            Some(orb_messages::main::mcu_to_jetson::Payload::Ack(ack)),
                    },
                )),
        } => {
            if is_ack_for_us(ack.ack_number) {
                ack_tx.send((CommonAckError::from(ack.error), ack.ack_number))?;
            } else {
                debug!("Ignoring ack # 0x{:x?}", ack.ack_number)
            }
        }
        orb_messages::McuMessage {
            version: _,
            message:
                Some(orb_messages::mcu_message::Message::MMessage(
                    orb_messages::main::McuToJetson { payload: Some(p) },
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
    message: &orb_messages::McuMessage,
    ack_tx: &mpsc::UnboundedSender<(CommonAckError, u32)>,
    new_message_queue: &mpsc::UnboundedSender<McuPayload>,
) -> Result<()> {
    match message {
        &orb_messages::McuMessage { version, .. }
            if version != orb_messages::sec::Version::Version0 as i32 =>
        {
            return Err(eyre!("unknown message version {:?}", version));
        }
        orb_messages::McuMessage {
            version: _,
            message:
                Some(orb_messages::mcu_message::Message::SecToJetsonMessage(
                    orb_messages::sec::SecToJetson {
                        payload:
                            Some(orb_messages::sec::sec_to_jetson::Payload::Ack(ack)),
                    },
                )),
        } => {
            if is_ack_for_us(ack.ack_number) {
                ack_tx.send((CommonAckError::from(ack.error), ack.ack_number))?;
            }
        }
        orb_messages::McuMessage {
            version: _,
            message:
                Some(orb_messages::mcu_message::Message::SecToJetsonMessage(
                    orb_messages::sec::SecToJetson { payload: Some(p) },
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
