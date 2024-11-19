use std::{
    convert::TryFrom,
    io,
    io::{Read, Write},
    time,
    time::{Duration, SystemTime},
};

use can::Id;
use eyre::{bail, ensure, eyre, WrapErr as _};
use orb_messages::{self as protobuf, prost::Message as _};
use orb_update_agent_core::{
    components,
    telemetry::{LogOnError, DATADOG},
    Slot,
};
use polling::{Event, Poller};
use tracing::{debug, info, warn};
use update_agent_can as can;
use update_agent_can::{
    isotp::{addr::CanIsotpAddr, stream::IsotpStream},
    CAN_DATA_LEN,
};

use super::Update;

/// ISO-TP addressing scheme on the Orb
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

impl TryFrom<u32> for IsoTpNodeIdentifier {
    type Error = eyre::Error;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            0x1 => Ok(IsoTpNodeIdentifier::MainMcu),
            0x2 => Ok(IsoTpNodeIdentifier::SecurityMcu),
            0x8 => Ok(IsoTpNodeIdentifier::Jetson),
            0x9 => Ok(IsoTpNodeIdentifier::JetsonApp1),
            0xA => Ok(IsoTpNodeIdentifier::JetsonApp2),
            0xB => Ok(IsoTpNodeIdentifier::JetsonApp3),
            0xC => Ok(IsoTpNodeIdentifier::JetsonApp4),
            0xD => Ok(IsoTpNodeIdentifier::JetsonApp5),
            0xE => Ok(IsoTpNodeIdentifier::JetsonApp6),
            0xF => Ok(IsoTpNodeIdentifier::JetsonApp7),
            _ => Err(eyre!("Unknown node id {value}")),
        }
    }
}

const UPDATE_AGENT_ISOTP_ID: IsoTpNodeIdentifier = IsoTpNodeIdentifier::JetsonApp2;
/// This constant is used to register interest in an event on the event poller. The API makes this
/// necessary but the value carries no extra meaning.
const ARBITRARY_EVENT_KEY: usize = 42;
/// MCU_MAX_FW_LEN_BYTES is 224KiB (per slot), the absolute maximum length
/// an MCU update can be. This is defined by the [MCU board DTS](https://github.com/worldcoin/orb-mcu-firmware/blob/d98719185b59375429123a5fd275dd5696a5bf12/boards/arm/mcu_main/mcu_main.dts#L516)
const MCU_MAX_FW_LEN_BYTES: u64 = 224 * 1024;
const MCU_BLOCK_LEN_BYTES: u64 = 39;
const MCU_BLOCK_SEND_ATTEMPTS: usize = 3;
/// 2.5s timeout for receiving an ack from the MCU
/// external SPI flash sector is long to erase.
const MCU_BLOCK_SEND_TIMEOUT_MS: u64 = 2500;
/// one block takes ~10ms to be sent over ISO-TP (with ack response)
/// let's use a maximum of 20% of the bandwidth when performing a microcontroller
/// firmware update so 10ms spaced by 40ms period
const MCU_BLOCK_SEND_THROTTLE_DELAY_MS: u64 = 40;

enum McuPayload {
    ToMain(protobuf::main::jetson_to_mcu::Payload),
    ToSec(protobuf::sec::jetson_to_sec::Payload),
}

/// Create ISO-TP pair of addresses, based on our addressing scheme
/// See docs for [`CAN_ADDR_IS_ISOTP`] & [`CAN_ADDR_IS_DEST`]
fn create_pair(
    src: IsoTpNodeIdentifier,
    dest: IsoTpNodeIdentifier,
) -> eyre::Result<(u32, u32)> {
    Ok((
        CAN_ADDR_IS_ISOTP | (src as u32) << 4 | dest as u32,
        CAN_ADDR_IS_DEST | CAN_ADDR_IS_ISOTP | (src as u32) << 4 | dest as u32,
    ))
}

#[derive(Debug)]
enum McuUpdateError {
    AckTimeout,
    Ack(i32),
    AckNumberMismatch,
    WriteError,
}

impl Update for components::Can {
    fn update<R>(&self, _slot: Slot, src: &mut R) -> eyre::Result<()>
    where
        R: io::Read + io::Seek + ?Sized,
    {
        DATADOG
            .incr("orb.update.count.component.can", ["status:started"])
            .or_log();
        src.seek(io::SeekFrom::Start(0))
            .wrap_err("failed to seek to start of CAN update source")?;
        let src_len = src
            .seek(io::SeekFrom::End(0))
            .wrap_err("failed to seek to end of CAN update source")?;
        src.seek(io::SeekFrom::Start(0))
            .expect("couldn't re-seek to start of CAN update source!");

        let block_len = ((src_len - 1) / MCU_BLOCK_LEN_BYTES + 1) as u32;
        debug!(
            "-- preparing to send {} block MCU update ({:?} bytes)",
            block_len, src_len
        );

        ensure!(
            src_len <= MCU_MAX_FW_LEN_BYTES,
            "hard check against maximum MCU firmware size failed with update of {} bytes",
            src_len,
        );

        // Maybe consolidate this with the length check
        let mut buffer = Vec::with_capacity(src_len as usize); // Safe cast
        src.read_to_end(&mut buffer)
            .wrap_err("failed reading CAN update source to end")?;

        let update_blocks = buffer.chunks(MCU_BLOCK_LEN_BYTES as usize);

        let remote = IsoTpNodeIdentifier::try_from(self.address)?;
        let mut update_stream =
            UpdateStream::new(UPDATE_AGENT_ISOTP_ID, remote, self.bus.clone())
                .wrap_err_with(|| {
                    eyre!(
                        "failed constructing update stream with {:?}, {:?}, {}",
                        UPDATE_AGENT_ISOTP_ID,
                        remote,
                        self.bus.clone()
                    )
                })?;

        debug!(
            "-- start sending mcu update to {:?}: {} blocks, {} bytes",
            remote, block_len, src_len,
        );
        for (blocks_num, block) in update_blocks.enumerate() {
            update_stream
                .send_block(block, blocks_num as u32, block_len)
                .wrap_err_with(|| {
                    eyre!("unable to send dfu block {}/{}", blocks_num, block_len)
                })
                .map_err({
                    DATADOG
                        .incr("orb.update.count.component.can", ["status:write_error"])
                        .or_log();
                    |e| e
                })?;
            std::thread::sleep(Duration::from_millis(MCU_BLOCK_SEND_THROTTLE_DELAY_MS));
        }

        // check CRC32 of sent firmware image
        let crc = crc32fast::hash(buffer.as_slice());
        let payload = match remote {
            IsoTpNodeIdentifier::MainMcu => McuPayload::ToMain(
                protobuf::main::jetson_to_mcu::Payload::FwImageCheck(
                    protobuf::FirmwareImageCheck { crc32: crc },
                ),
            ),
            IsoTpNodeIdentifier::SecurityMcu => {
                McuPayload::ToSec(protobuf::sec::jetson_to_sec::Payload::FwImageCheck(
                    protobuf::FirmwareImageCheck { crc32: crc },
                ))
            }
            _ => bail!("Unknown node"),
        };
        update_stream.send_payload(payload).map_err({
            DATADOG
                .incr(
                    "orb.update.count.component.can",
                    ["status:post_check_error"],
                )
                .or_log();
            |e| e
        })?;

        // activate image in MCU secondary slot so that the image is used
        // after reboot
        // the main microcontroller will wait for the Jetson to shutdown
        // and reboot itself to install the firmware upgrade
        let payload = match remote {
            IsoTpNodeIdentifier::MainMcu => McuPayload::ToMain(
                protobuf::main::jetson_to_mcu::Payload::FwImageSecondaryActivate(
                    protobuf::FirmwareActivateSecondary {
                        force_permanent: false,
                    },
                ),
            ),
            IsoTpNodeIdentifier::SecurityMcu => McuPayload::ToSec(
                protobuf::sec::jetson_to_sec::Payload::FwImageSecondaryActivate(
                    protobuf::FirmwareActivateSecondary {
                        force_permanent: false,
                    },
                ),
            ),
            _ => bail!("Unknown node"),
        };
        update_stream.send_payload(payload).map_err({
            DATADOG
                .incr(
                    "orb.update.count.component.can",
                    ["status:activation_error"],
                )
                .or_log();
            |e| e
        })?;

        // Security MCU won't reboot to install the new update
        // if we don't explicitly ask to reboot
        match remote {
            IsoTpNodeIdentifier::SecurityMcu => {
                let payload =
                    McuPayload::ToSec(protobuf::sec::jetson_to_sec::Payload::Reboot(
                        protobuf::RebootWithDelay { delay: 5 },
                    ));
                update_stream.send_payload(payload)?;
            }
            IsoTpNodeIdentifier::MainMcu => {}
            _ => bail!("Unknown node"),
        };

        DATADOG
            .incr("orb.update.count.component.can", ["status:write_complete"])
            .or_log();
        Ok(())
    }
}

struct UpdateStream {
    remote: IsoTpNodeIdentifier,
    tx_stream: IsotpStream<CAN_DATA_LEN>,
    ack_num: u32,
    // XXX: field order is significant here.
    //
    // Fields are dropped in declaration order. A stopping condition of the `_thread` is that
    // all receivers are dropped. This means that `ack_rx` *must* be dropped before `_thread` so
    // that `_thread` can drop without blocking.
    ack_rx: flume::Receiver<protobuf::Ack>,
    _thread: jod_thread::JoinHandle<eyre::Result<()>>,
}

impl UpdateStream {
    fn new(
        local: IsoTpNodeIdentifier,
        remote: IsoTpNodeIdentifier,
        bus: String,
    ) -> eyre::Result<UpdateStream> {
        let (ack_tx, ack_rx) = flume::unbounded();

        let (tx_stdid_src, tx_stdid_dst) = create_pair(local, remote)?;
        let tx_stream = IsotpStream::<CAN_DATA_LEN>::build()
            .bind(
                CanIsotpAddr::new(
                    bus.as_str(),
                    Id::Standard(tx_stdid_dst),
                    Id::Standard(tx_stdid_src),
                )
                .wrap_err_with(|| eyre!("failed to create ISO-TP addresses"))?,
            )
            .wrap_err_with(|| {
                eyre!("failed to bind to interface '{:?}'", bus.clone())
            })?;

        debug!(
            "-- bound tx socket on {:?}: 0x{:x}->0x{:x}",
            bus, tx_stdid_src, tx_stdid_dst
        );

        let _thread = jod_thread::spawn(move || {
            let (rx_stdid_src, rx_stdid_dest) = create_pair(remote, local)?;
            let stream = IsotpStream::<CAN_DATA_LEN>::build()
                .bind(
                    CanIsotpAddr::new(
                        bus.as_str(),
                        Id::Standard(rx_stdid_src),
                        Id::Standard(rx_stdid_dest),
                    )
                    .wrap_err_with(|| eyre!("failed to create ISO-TP addresses"))?,
                )
                .wrap_err_with(|| {
                    eyre!("failed to bind to interface '{:?}'", bus.clone())
                })?;
            debug!(
                "-- bound rx socket on {bus}: 0x{:x}->0x{:x}",
                rx_stdid_src, rx_stdid_dest
            );

            match UpdateStream::recv_ack(stream, ack_tx) {
                Ok(()) => {
                    info!("closing recv worker thread");
                    Ok(())
                }
                Err(e) => Err(e),
            }
        });
        Ok(UpdateStream {
            remote,
            tx_stream,
            ack_num: 0,
            ack_rx,
            _thread,
        })
    }

    fn send_block(
        &mut self,
        block: &[u8],
        block_num: u32,
        block_count: u32,
    ) -> eyre::Result<()> {
        let data = protobuf::FirmwareUpdateData {
            block_number: block_num,
            block_count,
            image_block: block.to_vec(),
        };

        let message = match self.remote {
            IsoTpNodeIdentifier::MainMcu => McuPayload::ToMain(
                protobuf::main::jetson_to_mcu::Payload::DfuBlock(data),
            ),
            IsoTpNodeIdentifier::SecurityMcu => {
                McuPayload::ToSec(protobuf::sec::jetson_to_sec::Payload::DfuBlock(data))
            }
            _ => bail!("unknown node"),
        };
        self.send_payload(message)
    }

    /// Send payload into McuMessage
    fn send_payload(&mut self, payload: McuPayload) -> eyre::Result<()> {
        let to_encode = match payload {
            McuPayload::ToMain(m) => protobuf::McuMessage {
                version: protobuf::Version::Version0 as i32,
                message: Some(protobuf::mcu_message::Message::JMessage(
                    protobuf::main::JetsonToMcu {
                        ack_number: self.ack_num,
                        payload: Some(m),
                    },
                )),
            },
            McuPayload::ToSec(s) => protobuf::McuMessage {
                version: protobuf::Version::Version0 as i32,
                message: Some(protobuf::mcu_message::Message::JetsonToSecMessage(
                    protobuf::sec::JetsonToSec {
                        ack_number: self.ack_num,
                        payload: Some(s),
                    },
                )),
            },
        };
        let bytes: Vec<u8> = to_encode.encode_length_delimited_to_vec();
        self.send_wait_ack_retry(bytes.as_slice(), MCU_BLOCK_SEND_ATTEMPTS)
            .map_err(|e| eyre!("message not sent {:?}, ack #{}", e, self.ack_num))?;

        // increase ack number for next payload to send
        self.ack_num += 1;
        Ok(())
    }

    fn send_wait_ack_retry(
        &mut self,
        frame: &[u8],
        retries: usize,
    ) -> Result<(), McuUpdateError> {
        let res = self.send_wait_ack(frame);
        match (retries, res) {
            (_, Ok(())) => Ok(()),
            (0, err @ Err(_)) => {
                warn!("failed after {MCU_BLOCK_SEND_ATTEMPTS} attempts: {err:?}");
                err
            }
            (_, Err(McuUpdateError::Ack(ack_error)))
                if ack_error == protobuf::ack::ErrorCode::Range as i32 =>
            {
                // block already received in a previous attempt
                // consider it as a success
                if retries < MCU_BLOCK_SEND_ATTEMPTS {
                    warn!("block already received by microcontroller? consider it as a success");
                    Ok(())
                } else {
                    Err(McuUpdateError::Ack(protobuf::ack::ErrorCode::Range as i32))
                }
            }
            (
                _,
                err @ Err(
                    McuUpdateError::AckTimeout
                    | McuUpdateError::AckNumberMismatch
                    | McuUpdateError::WriteError,
                ),
            ) => {
                warn!("sending ack-expectant frame failed, {retries} attempts left: {err:?}");
                // bus is busy? wait a bit and retry
                std::thread::sleep(Duration::from_millis(
                    MCU_BLOCK_SEND_THROTTLE_DELAY_MS * 2,
                ));
                self.send_wait_ack_retry(frame, retries - 1)
            }
            (_, err @ Err(_)) => err,
        }
    }

    fn wait_ack(&mut self) -> Result<(), McuUpdateError> {
        let start = SystemTime::now();
        let mut status: Result<(), McuUpdateError> = Err(McuUpdateError::AckTimeout);
        loop {
            if let Ok(ack) = self.ack_rx.try_recv() {
                if ack.ack_number == self.ack_num
                    && ack.error == protobuf::ack::ErrorCode::Success as i32
                {
                    return Ok(());
                } else if ack.ack_number == self.ack_num {
                    return Err(McuUpdateError::Ack(ack.error));
                } else {
                    status = Err(McuUpdateError::AckNumberMismatch)
                }
            }

            match start.elapsed() {
                Ok(elapsed)
                    if elapsed > Duration::from_millis(MCU_BLOCK_SEND_TIMEOUT_MS) =>
                {
                    return status;
                }
                _ => (),
            }
        }
    }

    fn send_wait_ack(&mut self, frame: &[u8]) -> Result<(), McuUpdateError> {
        self.ack_rx.drain().all(|_| true);
        let _ = self
            .tx_stream
            .write(frame)
            .map_err(|_| McuUpdateError::WriteError)?;
        self.wait_ack()
    }

    fn recv_ack(
        mut stream: IsotpStream<CAN_DATA_LEN>,
        ack_tx: flume::Sender<protobuf::Ack>,
    ) -> eyre::Result<()> {
        let poller = Poller::new().wrap_err("failed creating a new event poller")?;
        poller
            .add(&stream, Event::readable(ARBITRARY_EVENT_KEY))
            .wrap_err("failed adding can socket stream to event poller")?;
        let mut events = Vec::new();
        'eventloop: loop {
            events.clear();
            poller
                .wait(&mut events, Some(time::Duration::from_secs(1)))
                .wrap_err("error occured while waiting on event poller")?;
            for _event in &events {
                let mut buffer = [0; 1024];
                let size = stream
                    .read(&mut buffer)
                    .wrap_err("failed reading from CAN stream")?;
                poller
                    .modify(&stream, Event::readable(ARBITRARY_EVENT_KEY))
                    .wrap_err("failed setting interest for next socket read event")?;

                let try_message =
                    protobuf::McuMessage::decode_length_delimited(&buffer[..size]);
                let ack_msg = match try_message {
                    Ok(protobuf::McuMessage { version, .. })
                        if version != protobuf::Version::Version0 as i32 =>
                    {
                        warn!("received unknown version {:?}", version);
                        None
                    }

                    Ok(protobuf::McuMessage {
                        message:
                            Some(protobuf::mcu_message::Message::MMessage(
                                protobuf::main::McuToJetson {
                                    payload:
                                        Some(protobuf::main::mcu_to_jetson::Payload::Ack(
                                            ack,
                                        )),
                                },
                            )),
                        ..
                    }) => Some(ack),

                    Ok(protobuf::McuMessage {
                        message:
                            Some(protobuf::mcu_message::Message::SecToJetsonMessage(
                                protobuf::sec::SecToJetson {
                                    payload:
                                        Some(protobuf::sec::sec_to_jetson::Payload::Ack(
                                            ack,
                                        )),
                                },
                            )),
                        ..
                    }) => Some(ack),

                    Ok(_) => None,

                    Err(err) => {
                        return Err(err)
                            .wrap_err("failed decoding mcu protobuf message");
                    }
                };

                if let Some(ack_msg) = ack_msg {
                    if ack_msg.ack_number % 100 == 0 || ack_msg.error != 0 {
                        info!(
                            "received ack #{:?} (err {:?})...",
                            ack_msg.ack_number, ack_msg.error
                        );
                    }
                    if ack_tx.send(ack_msg).is_err() {
                        warn!(
                            "failed sending on ack channel: channel dropped all receivers; \
                             breaking event loop"
                        );
                        break 'eventloop;
                    }
                };
            }
            if ack_tx.is_disconnected() {
                info!("ack channel is disconnected; breaking event loop");
                break 'eventloop;
            }
        }
        Ok(())
    }
}

pub const RECOVERY_STATIC_FAN_SPEED_PERCENTAGE: u32 = 35;

pub fn try_mcu_set_static_fan_speed() -> eyre::Result<()> {
    let mcu_id = IsoTpNodeIdentifier::MainMcu;
    let mut update_stream =
        UpdateStream::new(UPDATE_AGENT_ISOTP_ID, mcu_id, "can0".to_string())
            .wrap_err_with(|| {
                eyre!(
            "failed constructing initial fan speed update stream with {:?}, {:?}, {}",
            UPDATE_AGENT_ISOTP_ID,
            mcu_id,
            "can0"
        )
            })?;

    let payload = McuPayload::ToMain(protobuf::main::jetson_to_mcu::Payload::FanSpeed(
        protobuf::main::FanSpeed {
            payload: Some(protobuf::main::fan_speed::Payload::Percentage(
                RECOVERY_STATIC_FAN_SPEED_PERCENTAGE,
            )),
        },
    ));

    update_stream.send_payload(payload).wrap_err_with(|| {
        eyre!(
            "failed setting static recovery fan speed `{:?}`",
            RECOVERY_STATIC_FAN_SPEED_PERCENTAGE
        )
    })?;

    Ok(())
}
