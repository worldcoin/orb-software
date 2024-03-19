use std::process;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::mpsc;

use async_trait::async_trait;
use eyre::{eyre, Context, Result};
use orb_messages::CommonAckError;
use prost::Message;
use tokio::time::Duration;
use tracing::debug;

use can_rs::filter::Filter;
use can_rs::stream::FrameStream;
use can_rs::{Frame, Id};

use crate::messaging::Device::{JetsonFromMain, JetsonFromSecurity, Main, Security};
use crate::messaging::{
    handle_main_mcu_message, handle_sec_mcu_message, Device, McuPayload,
    MessagingInterface,
};

pub struct CanRawMessaging {
    stream: FrameStream<64>,
    ack_num_lsb: AtomicU16,
    ack_queue: mpsc::Receiver<(CommonAckError, u32)>,
    can_node: Device,
}

impl CanRawMessaging {
    /// CanRawMessaging opens a CAN stream filtering messages addressed only to the Jetson
    /// and start listening for incoming messages in a new blocking thread
    pub fn new(
        bus: String,
        can_node: Device,
        new_message_queue: mpsc::Sender<McuPayload>,
    ) -> Result<Self> {
        // open socket
        let stream = FrameStream::<64>::build()
            .nonblocking(false)
            .filters(vec![
                Filter {
                    id: Id::Extended(JetsonFromMain as u32),
                    mask: 0xff,
                },
                Filter {
                    id: Id::Extended(JetsonFromSecurity as u32),
                    mask: 0xff,
                },
            ])
            .bind(bus.as_str().parse().unwrap())?;

        let (ack_tx, ack_rx) = mpsc::channel();

        // spawn CAN receiver
        let stream_copy = stream.try_clone()?;
        tokio::task::spawn(can_rx(stream_copy, can_node, ack_tx, new_message_queue));

        Ok(Self {
            stream,
            ack_num_lsb: AtomicU16::new(0),
            ack_queue: ack_rx,
            can_node,
        })
    }

    async fn wait_ack(&mut self, expected_ack_number: u32) -> Result<CommonAckError> {
        loop {
            match self.ack_queue.recv_timeout(Duration::from_millis(1500)) {
                Ok((ack, number)) => {
                    if number == expected_ack_number {
                        return Ok(ack);
                    }
                }
                Err(e) => {
                    return Err(eyre!("ack not received (raw): {}", e));
                }
            }
        }
    }

    #[allow(dead_code)]
    async fn send_wait_ack(&mut self, frame: &Frame<64>) -> Result<CommonAckError> {
        self.stream.send(frame, 0)?;

        // put some randomness into ack number to prevent collision with other processes
        let expected_ack_number =
            process::id() << 16 | self.ack_num_lsb.load(Ordering::Relaxed) as u32;
        self.ack_num_lsb.fetch_add(1, Ordering::Relaxed);

        self.wait_ack(expected_ack_number).await
    }
}

/// Receive CAN frames
/// - relay acks to `ack_tx`
/// - relay new McuMessage to `new_message_queue`
#[allow(dead_code)]
async fn can_rx(
    stream: FrameStream<64>,
    remote_node: Device,
    ack_tx: mpsc::Sender<(CommonAckError, u32)>,
    new_message_queue: mpsc::Sender<McuPayload>,
) -> Result<()> {
    loop {
        let mut frame: Frame<64> = Frame::empty();
        loop {
            match stream.recv(&mut frame, 0) {
                Ok(_) => {
                    break;
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(err) => return Err(eyre!("failed to read: {}", err)),
            }
        }
        let status = match remote_node {
            Main => {
                let message =
                    orb_messages::mcu_main::McuMessage::decode_length_delimited(
                        &frame.data[0..frame.len as usize],
                    )?;
                handle_main_mcu_message(&message, &ack_tx, &new_message_queue)
                    .wrap_err_with(|| "remote: main mcu")
            }
            Security => {
                let message =
                    orb_messages::mcu_sec::McuMessage::decode_length_delimited(
                        &frame.data[0..frame.len as usize],
                    )?;
                handle_sec_mcu_message(&message, &ack_tx, &new_message_queue)
                    .wrap_err_with(|| "remote: security mcu")
            }
            JetsonFromMain => Err(eyre!(
                "JetsonFromMain is not a valid destination for receiving messages"
            )),
            JetsonFromSecurity => Err(eyre!(
                "JetsonFromSecurity is not a valid destination for receiving messages"
            )),
        };

        if let Err(e) = status {
            debug!("Error handling message: {:#}", e);
        }
    }
}

#[async_trait]
impl MessagingInterface for CanRawMessaging {
    /// Send payload into McuMessage
    #[allow(dead_code)]
    async fn send(&mut self, payload: McuPayload) -> Result<CommonAckError> {
        // snowflake ack ID to avoid collisions:
        // prefix ack number with process ID
        let ack_number =
            process::id() << 16 | self.ack_num_lsb.load(Ordering::Relaxed) as u32;

        let bytes = match self.can_node {
            Main => {
                let to_encode = if let McuPayload::ToMain(p) = payload {
                    orb_messages::mcu_main::McuMessage {
                        version: orb_messages::mcu_main::Version::Version0 as i32,
                        message: Some(
                            orb_messages::mcu_main::mcu_message::Message::JMessage(
                                orb_messages::mcu_main::JetsonToMcu {
                                    ack_number,
                                    payload: Some(p),
                                },
                            ),
                        ),
                    }
                } else {
                    return Err(eyre!("Invalid payload type for main mcu node"));
                };
                Some(to_encode.encode_length_delimited_to_vec())
            }
            Security => {
                let to_encode = if let McuPayload::ToSec(p) = payload {
                    orb_messages::mcu_sec::McuMessage {
                        version: orb_messages::mcu_sec::Version::Version0 as i32,
                        message: Some(orb_messages::mcu_sec::mcu_message::Message::JetsonToSecMessage(
                            orb_messages::mcu_sec::JetsonToSec {
                                ack_number,
                                payload: Some(p),
                            },
                        )),
                    }
                } else {
                    return Err(eyre!("Invalid payload type for security mcu node"));
                };
                Some(to_encode.encode_length_delimited_to_vec())
            }
            JetsonFromMain => {
                return Err(eyre!(
                    "JetsonFromMain is not a valid destination for sending messages"
                ));
            }
            JetsonFromSecurity => {
                return Err(eyre!(
                    "JetsonFromSecurity is not a valid destination for sending messages"
                ));
            }
        };

        if let Some(bytes) = bytes {
            let mut buf: [u8; 64] = [0u8; 64];
            buf[..bytes.len()].copy_from_slice(bytes.as_slice());

            let node_addr = self.can_node as u32;
            let frame = Frame {
                id: Id::Extended(node_addr),
                len: 64,
                flags: 0x0F,
                data: buf,
            };

            self.send_wait_ack(&frame).await
        } else {
            Err(eyre!("Failed to encode payload"))
        }
    }
}

impl Drop for CanRawMessaging {
    fn drop(&mut self) {
        // TODO
    }
}
