use async_trait::async_trait;
use color_eyre::eyre::{eyre, Context, Result};
use futures::FutureExt as _;
use orb_messages::CommonAckError;
use prost::Message;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicU16, Ordering};
use tokio::sync::{mpsc, oneshot};
use tokio::time::timeout;
use tracing::{debug, error, trace};

use can_rs::isotp::addr::CanIsotpAddr;
use can_rs::isotp::stream::IsotpStream;
use can_rs::{Id, CAN_DATA_LEN};

use crate::{
    create_ack, handle_main_mcu_message, handle_sec_mcu_message, McuPayload,
    MessagingInterface,
};

use super::RX_TIMEOUT;

/// ISO-TP addressing scheme
/// 11-bit standard ID
/// | 10     | 9       | 8        |    [4-7]  |   [0-3]  |
/// | ------ | ------- | -------- | --------- | -------- |
/// | rsvd   | is_dest | is_isotp | source ID | dest ID  |
const CAN_ADDR_IS_ISOTP: u32 = 1 << 8;
const CAN_ADDR_IS_DEST: u32 = 1 << 9;

/// Hex digit used to identify the source or destination (source ID, dest ID) of a device or an app
/// Note. CAN Standard IDs are used on the CAN bus with ISO-TP and to bring maximum flexibility
/// for bidirectional communication, addresses are comprised of source and destination digit
/// along with some flags.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum IsoTpNodeIdentifier {
    MainMcu = 0x1,
    SecurityMcu = 0x2,
    Jetson = 0x8,
    /// core
    JetsonApp1 = 0x9,
    /// update-agent
    JetsonApp2 = 0xA,
    /// unused
    JetsonApp3 = 0xB,
    /// plug-and-trust
    JetsonApp4 = 0xC,
    JetsonApp5 = 0xD,
    JetsonApp6 = 0xE,
    /// mcu-util
    JetsonApp7 = 0xF,
}

impl From<u8> for IsoTpNodeIdentifier {
    fn from(value: u8) -> Self {
        match value {
            0x1 => IsoTpNodeIdentifier::MainMcu,
            0x2 => IsoTpNodeIdentifier::SecurityMcu,
            0x8 => IsoTpNodeIdentifier::Jetson,
            0x9 => IsoTpNodeIdentifier::JetsonApp1,
            0xA => IsoTpNodeIdentifier::JetsonApp2,
            0xB => IsoTpNodeIdentifier::JetsonApp3,
            0xC => IsoTpNodeIdentifier::JetsonApp4,
            0xD => IsoTpNodeIdentifier::JetsonApp5,
            0xE => IsoTpNodeIdentifier::JetsonApp6,
            0xF => IsoTpNodeIdentifier::JetsonApp7,
            _ => panic!("Invalid IsoTpNodeIdentifier {value}"),
        }
    }
}

pub struct CanIsoTpMessaging {
    stream: IsotpStream<CAN_DATA_LEN>,
    ack_num_lsb: AtomicU16,
    ack_queue: mpsc::UnboundedReceiver<(CommonAckError, u32)>,
    _kill_tx: oneshot::Sender<()>,
}

/// Create ISO-TP pair of addresses, based on our addressing scheme
fn create_pair(
    src: IsoTpNodeIdentifier,
    dest: IsoTpNodeIdentifier,
) -> Result<(u32, u32)> {
    Ok((
        CAN_ADDR_IS_ISOTP | (src as u32) << 4 | dest as u32,
        CAN_ADDR_IS_DEST | CAN_ADDR_IS_ISOTP | (src as u32) << 4 | dest as u32,
    ))
}

impl CanIsoTpMessaging {
    /// Opens a new CAN ISO-TP connection between two nodes. Two streams are created with two distinct
    /// pairs of addresses, one for transmission of ISO-TP messages and one for reception.
    /// A blocking thread is created for listening to new incoming messages.
    ///
    /// One pair of addresses _should_ be uniquely used on the bus to prevent misinterpretation of
    /// transmitted messages.
    /// If a pair of addresses is used by several programs, they must ensure one, and only one,
    /// does _not_ use the `IsotpFlags::ListenMode` whilst all the others do.
    pub fn new(
        bus: String,
        local: IsoTpNodeIdentifier,
        remote: IsoTpNodeIdentifier,
        new_message_queue: mpsc::Sender<McuPayload>,
    ) -> Result<CanIsoTpMessaging> {
        let (tx_stdid_src, tx_stdid_dst) = create_pair(local, remote)?;
        debug!("Sending on 0x{:x}->0x{:x}", tx_stdid_src, tx_stdid_dst);

        // open TX stream
        let tx_isotp_stream = IsotpStream::<CAN_DATA_LEN>::build()
            .bind(
                CanIsotpAddr::new(
                    bus.as_str(),
                    Id::Standard(tx_stdid_dst),
                    Id::Standard(tx_stdid_src),
                )
                .expect("Unable to build IsoTpStream"),
            )
            .wrap_err("Failed to bind CAN ISO-TP stream")?;

        let (ack_tx, ack_rx) = mpsc::unbounded_channel();
        let (kill_tx, kill_rx) = oneshot::channel();
        // spawn CAN receiver
        tokio::task::spawn_blocking(move || {
            can_rx(bus, remote, local, ack_tx, new_message_queue, kill_rx)
        });

        Ok(CanIsoTpMessaging {
            stream: tx_isotp_stream,
            ack_num_lsb: AtomicU16::new(0),
            ack_queue: ack_rx,
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
            .wrap_err("ack not received (isotp)")
    }

    async fn send_wait_ack(
        &mut self,
        frame: Vec<u8>,
        ack_number: u32,
    ) -> Result<CommonAckError> {
        let mut stream = self.stream.try_clone()?;
        tokio::task::spawn_blocking(move || -> Result<()> {
            let bytes = frame.as_slice();
            let nbytes_written = stream
                .write(bytes)
                .wrap_err("error while writing to isotp stream")?;
            trace!(
                "wrote {nbytes_written} bytes, for frame of length {}",
                frame.len()
            );
            Ok(())
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
    bus: String,
    remote: IsoTpNodeIdentifier,
    local: IsoTpNodeIdentifier,
    ack_tx: mpsc::UnboundedSender<(CommonAckError, u32)>,
    new_message_queue: mpsc::Sender<McuPayload>,
    mut kill_rx: oneshot::Receiver<()>,
) -> Result<()> {
    // rx messages <=> from remote to local
    let (rx_stdid_src, rx_stdid_dest) = create_pair(remote, local)?;
    debug!("Listening on 0x{:x}->0x{:x}", rx_stdid_src, rx_stdid_dest);

    let mut rx_isotp_stream = IsotpStream::<CAN_DATA_LEN>::build().bind(
        CanIsotpAddr::new(
            bus.as_str(),
            Id::Standard(rx_stdid_src),
            Id::Standard(rx_stdid_dest),
        )
        .expect("Unable to build IsoTpAddr"),
    )?;

    loop {
        let mut buffer = [0; 1024];

        // terminate task on kill signal
        use tokio::sync::oneshot::error::TryRecvError;
        match kill_rx.try_recv() {
            Ok(()) | Err(TryRecvError::Closed) => return Ok(()),
            Err(oneshot::error::TryRecvError::Empty) => (),
        }

        let buffer = match rx_isotp_stream.read(&mut buffer) {
            Ok(_) => buffer,
            Err(e) => {
                error!("Error reading from socket: {:?}", e);
                continue;
            }
        };

        let status = match remote {
            IsoTpNodeIdentifier::MainMcu => {
                let message =
                    orb_messages::mcu_main::McuMessage::decode_length_delimited(
                        buffer.as_slice(),
                    )?;
                handle_main_mcu_message(&message, &ack_tx, &new_message_queue)
                    .wrap_err_with(|| "remote: main mcu")
            }
            IsoTpNodeIdentifier::SecurityMcu => {
                let message =
                    orb_messages::mcu_sec::McuMessage::decode_length_delimited(
                        buffer.as_slice(),
                    )?;
                handle_sec_mcu_message(&message, &ack_tx, &new_message_queue)
                    .wrap_err_with(|| "remote: security mcu")
            }
            _ => Err(eyre!("Invalid destination: {:?}", local)),
        };

        if let Err(e) = status {
            debug!("Error handling message: {:#}", e);
        }
    }
}

#[async_trait]
impl MessagingInterface for CanIsoTpMessaging {
    /// Send payload into McuMessage
    /// One could decide to only listen for ISO-TP message so allow dead code for `send` method
    async fn send(&mut self, payload: McuPayload) -> Result<CommonAckError> {
        let ack_number = create_ack(self.ack_num_lsb.fetch_add(1, Ordering::SeqCst));

        let bytes = match payload {
            McuPayload::ToMain(p) => {
                let to_encode = orb_messages::mcu_main::McuMessage {
                    version: orb_messages::mcu_main::Version::Version0 as i32,
                    message: Some(
                        orb_messages::mcu_main::mcu_message::Message::JMessage(
                            orb_messages::mcu_main::JetsonToMcu {
                                ack_number,
                                payload: Some(p),
                            },
                        ),
                    ),
                };
                to_encode.encode_length_delimited_to_vec()
            }
            McuPayload::ToSec(p) => {
                let to_encode = orb_messages::mcu_sec::McuMessage {
                    version: orb_messages::mcu_sec::Version::Version0 as i32,
                    message: Some(
                        orb_messages::mcu_sec::mcu_message::Message::JetsonToSecMessage(
                            orb_messages::mcu_sec::JetsonToSec {
                                ack_number,
                                payload: Some(p),
                            },
                        ),
                    ),
                };
                to_encode.encode_length_delimited_to_vec()
            }
            _ => return Err(eyre!("Invalid payload")),
        };

        self.send_wait_ack(bytes, ack_number).await
    }
}
