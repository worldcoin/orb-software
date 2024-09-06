use crate::checks::mcu::Device::{JetsonFromMain, JetsonFromSecurity};
use can::stream::FrameStream;
use can::CANFD_DATA_LEN;
use can::{Frame, Id};
use eyre::WrapErr as _;
use log::warn;
use orb_messages::mcu_main as main_messages;
use orb_messages::mcu_sec as sec_messages;
use polling::{Event, Poller};
use prost::Message;
use std::{
    time,
    time::{Duration, SystemTime},
};
use tracing::{error, info};
use zbus::blocking::{Connection, Proxy};

const ARBITRARY_EVENT_KEY: usize = 1337;

const MCU_RESPONSE_TIMEOUT_MS: u64 = 800;
const MCU_SEND_RETRY_ATTEMPTS: usize = 3;
const MCU_SEND_RETRY_THROTTLE_DELAY_MS: u64 = 40;
const MCU_BACKUP_SHUTDOWN_DELAY_SEC: u32 = 30;

#[derive(Clone)]
enum Payload {
    ToMain(main_messages::jetson_to_mcu::Payload),
    ToSec(sec_messages::jetson_to_sec::Payload),
    FromMain(main_messages::mcu_to_jetson::Payload),
    FromSec(sec_messages::sec_to_jetson::Payload),
}

/// CAN(-FD) addressing scheme
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Device {
    Main = 0x01,
    Security = 0x02,
    JetsonFromMain = 0x80,
    JetsonFromSecurity = 0x81,
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(
        "Recoverable mismatched versions: expected: `{0}`, received: `{1}`, secondary slot: `{2}`"
    )]
    RecoverableVersionMismatch(String, String, String),

    #[error("Unable to perform recovery: {0}")]
    UnableToRecover(String),

    #[error("Defaulting to most recent version")]
    SecondaryIsMoreRecent(String),

    #[error("failed initializing message stream with {:?} on {:?}", .remote, .bus)]
    StreamInitialization {
        remote: Device,
        bus: String,
        error: StreamError,
    },

    #[error("failed sending message")]
    Stream(#[from] StreamError),

    #[error("encountered io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("failed to find variable '{0}' in both environment and `/etc/os-release`")]
    MissingExpectedVersion(String),

    #[error("encountered other mcu-related error: {0}")]
    Other(String),
}

pub struct Mcu {
    bus: String,
    remote: Device,
}

impl Mcu {
    pub fn main() -> Self {
        Self {
            bus: "can0".to_string(),
            remote: Device::Main,
        }
    }

    #[allow(dead_code)]
    pub fn sec() -> Self {
        Self {
            bus: "can0".to_string(),
            remote: Device::Security,
        }
    }

    /// Get versions in primary and secondary slots
    /// Returns a tuple of primary and secondary firmware versions
    /// Primary version is mandatory, otherwise an error is returned.
    fn get_versions(
        &self,
        mcu_stream: &mut MessageStream,
    ) -> Result<(semver::Version, Option<semver::Version>), Error> {
        let payload = match self.remote {
            Device::Main => {
                Payload::ToMain(main_messages::jetson_to_mcu::Payload::ValueGet(
                    main_messages::ValueGet {
                        value: main_messages::value_get::Value::FirmwareVersions as i32,
                    },
                ))
            }
            Device::Security => {
                Payload::ToSec(sec_messages::jetson_to_sec::Payload::ValueGet(
                    sec_messages::ValueGet {
                        value: main_messages::value_get::Value::FirmwareVersions as i32,
                    },
                ))
            }
            _ => unreachable!(),
        };

        mcu_stream.send_message(payload.clone())?;
        let resp = mcu_stream.recv_versions()?;

        let versions = match resp {
            Payload::FromMain(main_messages::mcu_to_jetson::Payload::Versions(v)) => {
                let primary_app = v
                    .primary_app
                    .ok_or(Error::Other("missing primary app version".to_string()))
                    .map(|v| {
                        semver::Version::new(
                            u64::from(v.major),
                            u64::from(v.minor),
                            u64::from(v.patch),
                        )
                    })?;
                let secondary_app = v.secondary_app.map(|v| {
                    semver::Version::new(
                        u64::from(v.major),
                        u64::from(v.minor),
                        u64::from(v.patch),
                    )
                });
                (primary_app, secondary_app)
            }
            Payload::FromSec(sec_messages::sec_to_jetson::Payload::Versions(v)) => {
                let primary_app = v
                    .primary_app
                    .ok_or(Error::Other("missing primary app version".to_string()))
                    .map(|v| {
                        semver::Version::new(
                            u64::from(v.major),
                            u64::from(v.minor),
                            u64::from(v.patch),
                        )
                    })?;
                let secondary_app = v.secondary_app.map(|v| {
                    semver::Version::new(
                        u64::from(v.major),
                        u64::from(v.minor),
                        u64::from(v.patch),
                    )
                });
                (primary_app, secondary_app)
            }
            _ => return Err(Error::Other("received incorrect message".to_string())),
        };

        Ok(versions)
    }

    fn expected_version(&self) -> Result<String, Error> {
        let var = match self.remote {
            Device::Main => "ORB_OS_EXPECTED_MAIN_MCU_VERSION",
            Device::Security => "ORB_OS_EXPECTED_SEC_MCU_VERSION",
            _ => unreachable!(),
        };

        std::env::var(var)
            .or_else(|_| {
                let release = std::fs::read_to_string("/etc/os-release")?;
                let var_prefix = format!("{var}=");
                release
                    .lines()
                    .find(|line| line.starts_with(&var_prefix))
                    .map(|line| line.trim_start_matches(&var_prefix).to_string())
                    .ok_or(Error::MissingExpectedVersion(var.to_string()))
            })
            .map_err(Into::into)
    }

    pub fn reboot_for_update(&self) -> Result<(), Error> {
        let mut mcu_stream =
            MessageStream::new(self.remote, &self.bus).map_err(|err| {
                Error::StreamInitialization {
                    remote: self.remote,
                    bus: self.bus.clone(),
                    error: err,
                }
            })?;

        // activate secondary slot in case not done already
        match self.remote {
            Device::Main => {
                mcu_stream.send_message(Payload::ToMain(
                    main_messages::jetson_to_mcu::Payload::FwImageSecondaryActivate(
                        main_messages::FirmwareActivateSecondary {
                            force_permanent: false,
                        },
                    ),
                ))?;
            }
            Device::Security => {
                mcu_stream.send_message(Payload::ToSec(
                    sec_messages::jetson_to_sec::Payload::FwImageSecondaryActivate(
                        sec_messages::FirmwareActivateSecondary {
                            force_permanent: false,
                        },
                    ),
                ))?;
            }
            _ => {
                unreachable!("unexpected device");
            }
        }

        if self.remote == Device::Main {
            // in case Jetson shutdown doesn't work, ask the MCU to reboot.
            mcu_stream
                .send_message(Payload::ToMain(
                    main_messages::jetson_to_mcu::Payload::Reboot(
                        main_messages::RebootWithDelay {
                            delay: MCU_BACKUP_SHUTDOWN_DELAY_SEC,
                        },
                    ),
                ))
                .map_err(Error::Stream)?;

            // trigger jetson shutdown so that the MCU takes the update
            trigger_shutdown()
                .map_err(|err| Error::SecondaryIsMoreRecent(err.to_string()))
        } else {
            // reboot security mcu
            mcu_stream
                .send_message(Payload::ToSec(
                    sec_messages::jetson_to_sec::Payload::Reboot(
                        sec_messages::RebootWithDelay { delay: 3 },
                    ),
                ))
                .map_err(Error::Stream)
        }
    }
}

impl super::Check for Mcu {
    type Error = Error;

    const NAME: &'static str = "mcu version";

    /// Checks microcontroller firmware versions after an update.
    /// Two slots are used on the microcontroller to store firmware images: the primary (running image)
    /// and secondary. Slots are switched during an update.
    ///
    /// The check is performed in three consecutive steps:
    /// 1. If the primary slot doesn't match the expected version, the secondary slot is checked to
    /// see if the expected version is there. If it is, the image is activated for an update on MCU reboot.
    /// The device is thus rebooted instantly to switch to the secondary slot.
    /// 2. If the expected version is not found in the secondary slot, a loose version check is performed
    /// by using only the major and minor number. The slot is selected accordingly and the device is
    /// rebooted if the best image is in secondary slot.
    /// 3. If none of the above, the most recent version is used by comparing the semver.
    fn check(&self) -> Result<(), Self::Error> {
        let expected_version =
            semver::Version::parse(self.expected_version()?.trim_start_matches('v'))
                .map_err(|err| Error::Other(err.to_string()))?;

        let mut mcu_stream =
            MessageStream::new(self.remote, &self.bus).map_err(|err| {
                Error::StreamInitialization {
                    remote: self.remote,
                    bus: self.bus.clone(),
                    error: err,
                }
            })?;

        let (primary_app, secondary_app) = self.get_versions(&mut mcu_stream)?;
        info!(
            "Mcu primary app: {:?}, secondary app: {:?}, expected: {}",
            primary_app, secondary_app, expected_version
        );

        if expected_version == primary_app {
            info!("Mcu primary app matches expected version");
            return Ok(());
        }

        if let Some(secondary_app) = secondary_app {
            if expected_version != primary_app && expected_version == secondary_app {
                info!("Mcu app in secondary slot matches expected version");
                return Err(Error::RecoverableVersionMismatch(
                    format!("{primary_app:?}"),
                    format!("{secondary_app:?}"),
                    format!("{expected_version:?}"),
                ));
            }
            // loose checks
            if expected_version.major == primary_app.major
                && expected_version.minor == primary_app.minor
            {
                info!("Primary app matches expected version loosely, so let's use it");
            } else if expected_version.major == secondary_app.major
                && expected_version.minor == secondary_app.minor
            {
                info!("Secondary app matches expected version loosely");
                return Err(Error::RecoverableVersionMismatch(
                    format!("{primary_app:?}"),
                    format!("{secondary_app:?}"),
                    format!("{expected_version:?}"),
                ));
            } else {
                warn!("Defaulting to most recent version...");
                if primary_app < secondary_app {
                    return Err(Error::SecondaryIsMoreRecent(
                        "Secondary slot is more recent".to_string(),
                    ));
                }
            }
        } else if expected_version.major == primary_app.major
            && expected_version.minor == primary_app.minor
        {
            info!("Mcu primary app matches expected version loosely in absence of secondary slot data, so let's continue with this version");
        } else {
            // far off
            return Err(Error::UnableToRecover(format!(
                "Primary app version {primary_app:?} is far off from expected version {expected_version:?}"
            )));
        }

        Ok(())
    }
}

fn trigger_shutdown() -> eyre::Result<()> {
    let connection = Connection::system()?;

    let proxy: Proxy<'_> = zbus::blocking::proxy::Builder::new(&connection)
        .interface("org.freedesktop.login1.Manager")?
        .path("/org/freedesktop/login1")?
        .destination("org.freedesktop.login1")?
        .build()?;

    // perform the shutdown right now (body is `true`)
    proxy.call_method("PowerOff", &true)?;

    Ok(())
}

#[derive(thiserror::Error, Debug)]
pub enum StreamError {
    #[error("could not init message stream: {0}: `{1}`")]
    Initialization(String, can::Error),
    #[error("timed out waiting to receive ack")]
    AckTimeout,
    #[error("timed out waiting for reply")]
    ReplyTimeout,
    #[error("received mismatched ack number")]
    AckMismatch,
    #[error("message sending failed")]
    WriteError(#[from] std::io::Error),
    #[error("message receiving failed")]
    ReceiveQueueError(#[from] flume::TryRecvError),
    #[error("received ack with error")]
    Ack(i32),
}

struct MessageStream {
    remote: Device,
    stream: FrameStream<CANFD_DATA_LEN>,
    ack_num: u32,
    // XXX: field order is significant here.
    //
    // Fields are dropped in declaration order. A stopping condition of the `_thread` is that
    // all receivers are dropped. This means that `ack_rx` *must* be dropped before `_thread` so
    // that `_thread` can drop without blocking.
    ack_rx: flume::Receiver<Payload>,
    msg_rx: flume::Receiver<Payload>,
    _thread: jod_thread::JoinHandle<eyre::Result<()>>,
}

impl MessageStream {
    fn new(remote: Device, bus: &str) -> Result<Self, StreamError> {
        let (ack_tx, ack_rx) = flume::unbounded();
        let (msg_tx, msg_rx) = flume::unbounded();

        let stream = FrameStream::<CANFD_DATA_LEN>::build()
            .nonblocking(true)
            .filters(vec![
                can::filter::Filter {
                    id: Id::Extended(JetsonFromMain as u32),
                    mask: 0xff,
                },
                can::filter::Filter {
                    id: Id::Extended(JetsonFromSecurity as u32),
                    mask: 0xff,
                },
            ])
            .bind(bus.parse().unwrap())
            .map_err(|err| {
                StreamError::Initialization(
                    format!("failed to bind to interface '{bus:?}'"),
                    err,
                )
            })?;

        let stream_copy = stream.try_clone().map_err(|err| {
            StreamError::Initialization(
                "failed to clone stream for worker thread".to_string(),
                err,
            )
        })?;
        let thread = jod_thread::spawn(move || {
            match MessageStream::rx_task(&stream_copy, remote, &ack_tx, &msg_tx) {
                Ok(()) => {
                    info!("closing recv worker thread");
                    Ok(())
                }
                Err(e) => Err(e),
            }
        });
        Ok(MessageStream {
            remote,
            stream,
            msg_rx,
            ack_num: 0,
            ack_rx,
            _thread: thread,
        })
    }

    /// Send payload to the microcontroller
    fn send_message(&mut self, payload: Payload) -> Result<(), StreamError> {
        let bytes = match payload {
            Payload::ToMain(m) => {
                let to_encode: orb_messages::mcu_main::McuMessage =
                    main_messages::McuMessage {
                        version: main_messages::Version::Version0 as i32,
                        message: Some(main_messages::mcu_message::Message::JMessage(
                            main_messages::JetsonToMcu {
                                ack_number: self.ack_num,
                                payload: Some(m),
                            },
                        )),
                    };
                to_encode.encode_length_delimited_to_vec()
            }
            Payload::ToSec(s) => {
                let to_encode = sec_messages::McuMessage {
                    version: sec_messages::Version::Version0 as i32,
                    message: Some(
                        sec_messages::mcu_message::Message::JetsonToSecMessage(
                            sec_messages::JetsonToSec {
                                ack_number: self.ack_num,
                                payload: Some(s),
                            },
                        ),
                    ),
                };
                to_encode.encode_length_delimited_to_vec()
            }
            _ => {
                unreachable!("send must use ToMain or ToSec payloads")
            }
        };

        // build frame
        let mut buf: [u8; CANFD_DATA_LEN] = [0u8; CANFD_DATA_LEN];
        buf[..bytes.len()].copy_from_slice(bytes.as_slice());
        let frame = Frame {
            id: Id::Extended(self.remote as u32),
            len: 64,
            flags: 0x0F,
            data: buf,
        };
        self.send_wait_ack_retry(&frame, MCU_SEND_RETRY_ATTEMPTS)?;

        // increase ack number for next payload to send
        self.ack_num += 1;
        Ok(())
    }

    fn send_wait_ack_retry(
        &mut self,
        frame: &Frame<CANFD_DATA_LEN>,
        retries: usize,
    ) -> Result<(), StreamError> {
        let res = self.send_wait_ack(frame);
        match (retries, res) {
            (_, Ok(())) => Ok(()),
            (0, err @ Err(_)) => {
                warn!("failed after {MCU_SEND_RETRY_ATTEMPTS} attempts: {err:?}");
                err
            }
            (_, Err(StreamError::Ack(ack_error)))
                if ack_error == main_messages::ack::ErrorCode::Range as i32 =>
            {
                Err(StreamError::Ack(
                    main_messages::ack::ErrorCode::Range as i32,
                ))
            }
            (
                _,
                err @ Err(
                    StreamError::AckTimeout
                    | StreamError::AckMismatch
                    | StreamError::WriteError(_),
                ),
            ) => {
                warn!("sending ack-expectant frame failed, {retries} attempts left: {err:?}");
                // bus is busy? wait a bit and retry
                std::thread::sleep(Duration::from_millis(
                    MCU_SEND_RETRY_THROTTLE_DELAY_MS * 2,
                ));
                self.send_wait_ack_retry(frame, retries - 1)
            }
            (_, err @ Err(_)) => err,
        }
    }

    fn wait_ack(&mut self) -> Result<(), StreamError> {
        let start = SystemTime::now();
        let mut status: Result<(), StreamError> = Err(StreamError::AckTimeout);
        loop {
            match self.ack_rx.try_recv() {
                Ok(Payload::FromMain(main_messages::mcu_to_jetson::Payload::Ack(
                    ack,
                ))) if self.remote == Device::Main => {
                    if ack.ack_number == self.ack_num
                        && ack.error == main_messages::ack::ErrorCode::Success as i32
                    {
                        return Ok(());
                    } else if ack.ack_number == self.ack_num {
                        return Err(StreamError::Ack(ack.error));
                    }
                    status = Err(StreamError::AckMismatch);
                }
                Ok(Payload::FromSec(sec_messages::sec_to_jetson::Payload::Ack(
                    ack,
                ))) if self.remote == Device::Security => {
                    if ack.ack_number == self.ack_num
                        && ack.error == sec_messages::ack::ErrorCode::Success as i32
                    {
                        return Ok(());
                    } else if ack.ack_number == self.ack_num {
                        return Err(StreamError::Ack(ack.error));
                    }
                    status = Err(StreamError::AckMismatch);
                }
                _ => {}
            };

            match start.elapsed() {
                Ok(elapsed)
                    if elapsed > Duration::from_millis(MCU_RESPONSE_TIMEOUT_MS) =>
                {
                    return status;
                }
                _ => (),
            }
        }
    }

    fn send_wait_ack(
        &mut self,
        frame: &Frame<CANFD_DATA_LEN>,
    ) -> Result<(), StreamError> {
        self.ack_rx.drain().all(|_| true);
        let _ = self
            .stream
            .send(frame, 0)
            .map_err(StreamError::WriteError)?;

        self.wait_ack()
    }

    /// Receive messages from the CAN bus and send them to the appropriate channel
    fn rx_task(
        stream: &FrameStream<CANFD_DATA_LEN>,
        remote: Device,
        ack_tx: &flume::Sender<Payload>,
        msg_tx: &flume::Sender<Payload>,
    ) -> eyre::Result<()> {
        let poller = Poller::new().wrap_err("failed creating a new event poller")?;
        poller
            .add(stream, Event::readable(ARBITRARY_EVENT_KEY))
            .wrap_err("failed adding can socket stream to event poller")?;
        let mut events = Vec::new();
        'eventloop: loop {
            events.clear();
            poller
                .wait(&mut events, Some(time::Duration::from_secs(1)))
                .wrap_err("error occurred while waiting on event poller")?;
            for _event in &events {
                let mut frame: Frame<CANFD_DATA_LEN> = Frame::empty();
                let _ = stream
                    .recv(&mut frame, 0)
                    .wrap_err("failed reading from CAN stream")?;
                poller
                    .modify(stream, Event::readable(ARBITRARY_EVENT_KEY))
                    .wrap_err("failed setting interest for next socket read event")?;

                let payload: Option<Payload> = decode_protobuf(remote, frame);

                match payload.clone() {
                    Some(Payload::FromMain(
                        main_messages::mcu_to_jetson::Payload::Ack(_ack),
                    )) => {
                        if ack_tx.send(payload.unwrap()).is_err() {
                            warn!(
                                "failed sending on ack channel: channel dropped all receivers; \
                             breaking event loop"
                            );
                            break 'eventloop;
                        }
                    }
                    Some(Payload::FromSec(
                        sec_messages::sec_to_jetson::Payload::Ack(_ack),
                    )) => {
                        if ack_tx.send(payload.unwrap()).is_err() {
                            warn!(
                                "failed sending on ack channel: channel dropped all receivers; \
                             breaking event loop"
                            );
                            break 'eventloop;
                        }
                    }
                    Some(Payload::FromMain(p)) if remote == Device::Main => {
                        msg_tx.send(Payload::FromMain(p.clone()))?;
                    }
                    Some(Payload::FromSec(p)) if remote == Device::Security => {
                        msg_tx.send(Payload::FromSec(p.clone()))?;
                    }
                    Some(_) | None => {}
                };
            }
            if ack_tx.is_disconnected() {
                info!("ack channel is disconnected; breaking event loop");
                break 'eventloop;
            }
        }
        Ok(())
    }

    fn recv_versions(&mut self) -> Result<Payload, StreamError> {
        let start = SystemTime::now();
        loop {
            let message = self.msg_rx.try_recv()?;

            match message {
                Payload::FromMain(main_messages::mcu_to_jetson::Payload::Versions(
                    _,
                )) if self.remote == Device::Main => return Ok(message),
                Payload::FromSec(sec_messages::sec_to_jetson::Payload::Versions(_))
                    if self.remote == Device::Security =>
                {
                    return Ok(message)
                }
                _ => {}
            };

            match start.elapsed() {
                Ok(elapsed)
                    if elapsed > Duration::from_millis(MCU_RESPONSE_TIMEOUT_MS) =>
                {
                    return Err(StreamError::ReplyTimeout);
                }
                _ => (),
            }
        }
    }
}

fn decode_protobuf(remote: Device, frame: Frame<CANFD_DATA_LEN>) -> Option<Payload> {
    match remote {
        Device::Main => {
            let message = main_messages::McuMessage::decode_length_delimited(
                &frame.data[0..frame.len as usize],
            );

            if let Ok(main_messages::McuMessage {
                version,
                message:
                    Some(main_messages::mcu_message::Message::MMessage(
                        main_messages::McuToJetson { payload: Some(p) },
                    )),
            }) = message
            {
                if version == main_messages::Version::Version0 as i32 {
                    Some(Payload::FromMain(p.clone()))
                } else {
                    warn!("received unknown version {:?}", version);
                    None
                }
            } else {
                None
            }
        }
        Device::Security => {
            let message = sec_messages::McuMessage::decode_length_delimited(
                &frame.data[0..frame.len as usize],
            );

            if let Ok(sec_messages::McuMessage {
                version,
                message:
                    Some(sec_messages::mcu_message::Message::SecToJetsonMessage(
                        sec_messages::SecToJetson { payload: Some(p) },
                    )),
            }) = message
            {
                if version == sec_messages::Version::Version0 as i32 {
                    Some(Payload::FromSec(p.clone()))
                } else {
                    warn!("received unknown version {:?}", version);
                    None
                }
            } else {
                None
            }
        }
        _ => None,
    }
}
