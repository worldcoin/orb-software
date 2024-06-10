use async_trait::async_trait;
use can_rs::filter::Filter;
use can_rs::stream::FrameStream;
use can_rs::{Frame, Id, CANFD_DATA_LEN};
use color_eyre::eyre::{eyre, Context, Result};
use futures::FutureExt as _;
use orb_messages::CommonAckError;
use prost::Message;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use tokio::time::timeout;
use tracing::{debug, trace};

use crate::Device::{JetsonFromMain, JetsonFromSecurity, Main, Security};
use crate::{
    create_ack, handle_main_mcu_message, handle_sec_mcu_message, Device, McuPayload,
    MessagingInterface,
};

use super::RX_TIMEOUT;

pub struct CanRawMessaging {
    stream: FrameStream<CANFD_DATA_LEN>,
    ack_num_lsb: AtomicU16,
    ack_queue: mpsc::UnboundedReceiver<(CommonAckError, u32)>,
    can_node: Device,
    /// Ensures that the task is killed when Self is dropped.
    _kill_tx: oneshot::Sender<()>,
}

impl CanRawMessaging {
    /// CanRawMessaging opens a CAN stream filtering messages addressed only to the Jetson
    /// and start listening for incoming messages in a new blocking thread
    pub fn new(
        bus: String,
        can_node: Device,
        new_message_queue: mpsc::UnboundedSender<McuPayload>,
    ) -> Result<Self> {
        // open socket
        let stream = FrameStream::<CANFD_DATA_LEN>::build()
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
            .bind(bus.as_str().parse().unwrap())
            .wrap_err("Failed to bind CAN stream")?;

        let (ack_tx, ack_rx) = mpsc::unbounded_channel();
        let (kill_tx, kill_rx) = oneshot::channel();
        let stream_copy = stream.try_clone()?;
        tokio::task::spawn_blocking(move || {
            can_rx(stream_copy, can_node, ack_tx, new_message_queue, kill_rx)
        });

        Ok(Self {
            stream,
            ack_num_lsb: AtomicU16::new(0),
            ack_queue: ack_rx,
            can_node,
            _kill_tx: kill_tx,
        })
    }

    async fn wait_ack(&mut self, expected_ack_number: u32) -> Result<CommonAckError> {
        let recv_fut = async {
            while let Some((ack, number)) = self.ack_queue.recv().await {
                if number == expected_ack_number {
                    return Ok(ack);
                }
            }

            Err(eyre!("ack queue closed"))
        };
        timeout(RX_TIMEOUT, recv_fut)
            .map(|result| result?)
            .await
            .wrap_err("ack not received (raw)")
    }

    async fn send_wait_ack(
        &mut self,
        frame: Arc<Frame<CANFD_DATA_LEN>>,
        ack_number: u32,
    ) -> Result<CommonAckError> {
        let stream = self.stream.try_clone()?;
        tokio::task::spawn_blocking(move || {
            let nbytes_written = stream
                .send(&frame, 0)
                .wrap_err("error while writing to canfd stream")?;
            trace!(
                "wrote {nbytes_written} bytes, for frame.len = {}, frame.data.len = {}",
                frame.len,
                frame.data.len(),
            );
            Ok::<(), color_eyre::Report>(())
        })
        .await
        .wrap_err("send_wait_ack task panicked")??;

        self.wait_ack(ack_number).await
    }
}

/// Receive CAN frames
/// - relay acks to `ack_tx`
/// - relay new McuMessage to `new_message_queue`
fn can_rx(
    stream: FrameStream<CANFD_DATA_LEN>,
    remote_node: Device,
    ack_tx: mpsc::UnboundedSender<(CommonAckError, u32)>,
    new_message_queue: mpsc::UnboundedSender<McuPayload>,
    mut kill_rx: oneshot::Receiver<()>,
) -> Result<()> {
    loop {
        let mut frame: Frame<CANFD_DATA_LEN> = Frame::empty();
        loop {
            // terminate task on kill signal
            use tokio::sync::oneshot::error::TryRecvError;
            match kill_rx.try_recv() {
                Ok(()) | Err(TryRecvError::Closed) => return Ok(()),
                Err(oneshot::error::TryRecvError::Empty) => (),
            }

            match stream.recv(&mut frame, 0) {
                Ok(_nbytes) => {
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
    async fn send(&mut self, payload: McuPayload) -> Result<CommonAckError> {
        let ack_number = create_ack(self.ack_num_lsb.fetch_add(1, Ordering::SeqCst));

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
            let mut buf = [0u8; CANFD_DATA_LEN];
            buf[..bytes.len()].copy_from_slice(bytes.as_slice());

            let node_addr = self.can_node as u32;
            let frame = Frame {
                id: Id::Extended(node_addr),
                len: bytes.len() as u8,
                flags: can_rs::CANFD_BRS_FLAG | can_rs::CANFD_FDF_FLAG,
                data: buf,
            };

            self.send_wait_ack(Arc::new(frame), ack_number).await
        } else {
            Err(eyre!("Failed to encode payload"))
        }
    }
}
