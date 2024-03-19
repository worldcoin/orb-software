use crate::messaging::{Device, McuPayload, MessagingInterface};
use async_trait::async_trait;
use eyre::{eyre, Result};
use orb_messages::CommonAckError;
use prost::Message;
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::Duration;
use std::vec;
use tokio::io::AsyncWriteExt;
use tokio::time;
use tokio_serial::{SerialPort, SerialPortBuilderExt, SerialStream};
use tracing::debug;

pub struct SerialMessaging {
    device: Device,
    port: SerialStream,
    ack_num_lsb: AtomicU16,
}

impl SerialMessaging {
    pub fn new(device: Device) -> Result<Self> {
        let mut port = match device {
            Device::Main => {
                tokio_serial::new("/dev/ttyTHS0", 1000000).open_native_async()?
            }
            Device::Security => {
                tokio_serial::new("/dev/ttyTHS1", 1000000).open_native_async()?
            }
            Device::JetsonFromMain | Device::JetsonFromSecurity => {
                return Err(eyre!("Cannot open serial from Jetson to Jetson"));
            }
        };

        port.set_data_bits(tokio_serial::DataBits::Eight)?;
        port.set_stop_bits(tokio_serial::StopBits::One)?;
        port.set_parity(tokio_serial::Parity::None)?;

        Ok(Self {
            device,
            port,
            ack_num_lsb: AtomicU16::new(0),
        })
    }
}

#[async_trait]
impl MessagingInterface for SerialMessaging {
    async fn send(&mut self, payload: McuPayload) -> Result<CommonAckError> {
        let ack_number = self.ack_num_lsb.load(Ordering::Relaxed) as u32;
        let mut payload = match self.device {
            Device::Main => {
                let payload = if let McuPayload::ToMain(payload) = payload {
                    payload
                } else {
                    return Err(eyre!("Invalid payload for Main"));
                };
                let to_encode = orb_messages::mcu_main::McuMessage {
                    version: orb_messages::mcu_main::Version::Version0 as i32,
                    message: Some(
                        orb_messages::mcu_main::mcu_message::Message::JMessage(
                            orb_messages::mcu_main::JetsonToMcu {
                                ack_number,
                                payload: Some(payload),
                            },
                        ),
                    ),
                };
                to_encode.encode_length_delimited_to_vec()
            }
            Device::Security => {
                let payload = if let McuPayload::ToSec(payload) = payload {
                    payload
                } else {
                    return Err(eyre!("Invalid payload for Main"));
                };

                let to_encode = orb_messages::mcu_sec::McuMessage {
                    version: orb_messages::mcu_sec::Version::Version0 as i32,
                    message: Some(
                        orb_messages::mcu_sec::mcu_message::Message::JetsonToSecMessage(
                            orb_messages::mcu_sec::JetsonToSec {
                                ack_number,
                                // cast payload to the correct type
                                payload: Some(payload),
                            },
                        ),
                    ),
                };
                to_encode.encode_length_delimited_to_vec()
            }
            _ => {
                panic!("Invalid device: {:?}", self.device)
            }
        };

        // UART message: magic (2B) + size (2B) + payload (protobuf-encoded McuMessage)
        let mut size = Vec::from((payload.len() as u16).to_le_bytes());
        let mut bytes: Vec<u8> = vec![0x8e, 0xad];
        bytes.append(&mut size);
        bytes.append(&mut payload);

        debug!("Sending {} bytes: {:?}", bytes.len(), bytes);

        self.port
            .write_all(bytes.as_slice())
            .await
            .expect("unable to write test message");

        time::sleep(Duration::from_millis(8)).await;

        Ok(CommonAckError::Success)
    }
}
