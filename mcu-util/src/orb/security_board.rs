use async_trait::async_trait;
use color_eyre::eyre::{eyre, Result, WrapErr as _};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time;
use tracing::{debug, error, info, warn};

use orb_mcu_interface::can::canfd::CanRawMessaging;
use orb_mcu_interface::can::isotp::{CanIsoTpMessaging, IsoTpNodeIdentifier};
use orb_mcu_interface::orb_messages;
use orb_mcu_interface::{Device, McuPayload, MessagingInterface};
use orb_messages::battery_status::BatteryState;
use orb_messages::{sec as security_messaging, CommonAckError};

use crate::orb::dfu::BlockIterator;
use crate::orb::{dfu, BatteryStatus};
use crate::orb::{Board, OrbInfo};

use super::BoardTaskHandles;

const REBOOT_DELAY: u32 = 3;

pub struct SecurityBoard {
    canfd_iface: CanRawMessaging,
    isotp_iface: CanIsoTpMessaging,
    message_queue_rx: mpsc::UnboundedReceiver<McuPayload>,
    canfd: bool,
}

pub struct SecurityBoardBuilder {
    message_queue_rx: mpsc::UnboundedReceiver<McuPayload>,
    message_queue_tx: mpsc::UnboundedSender<McuPayload>,
}

impl SecurityBoardBuilder {
    pub(crate) fn new() -> Self {
        let (message_queue_tx, message_queue_rx) =
            mpsc::unbounded_channel::<McuPayload>();

        Self {
            message_queue_rx,
            message_queue_tx,
        }
    }

    pub async fn build(self, canfd: bool) -> Result<(SecurityBoard, BoardTaskHandles)> {
        let (canfd_iface, raw_can_task) = CanRawMessaging::new(
            String::from("can0"),
            Device::Security,
            self.message_queue_tx.clone(),
        )
        .wrap_err("Failed to create CanRawMessaging for SecurityBoard")?;

        let (isotp_iface, isotp_can_task) = CanIsoTpMessaging::new(
            String::from("can0"),
            IsoTpNodeIdentifier::JetsonApp7,
            IsoTpNodeIdentifier::SecurityMcu,
            self.message_queue_tx.clone(),
        )
        .wrap_err("Failed to create CanIsoTpMessaging for SecurityBoard")?;

        Ok((
            SecurityBoard {
                canfd_iface,
                isotp_iface,
                message_queue_rx: self.message_queue_rx,
                canfd,
            },
            BoardTaskHandles {
                raw: raw_can_task,
                isotp: isotp_can_task,
            },
        ))
    }
}

impl SecurityBoard {
    pub fn builder() -> SecurityBoardBuilder {
        SecurityBoardBuilder::new()
    }

    /// Send a message to the security board with preferred interface
    pub async fn send(&mut self, payload: McuPayload) -> Result<CommonAckError> {
        if matches!(payload, McuPayload::ToSec(_)) {
            tracing::trace!(
                "sending message to security mcu over {}: {:?}",
                if self.canfd { "can-fd" } else { "iso-tp" },
                payload
            );
            if self.canfd {
                self.canfd_iface.send(payload).await
            } else {
                self.isotp_iface.send(payload).await
            }
        } else {
            Err(eyre!(
                "Message not targeted to security board: {:?}",
                payload
            ))
        }
    }

    pub async fn power_cycle_secure_element(&mut self) -> Result<()> {
        match self
            .send(McuPayload::ToSec(
                security_messaging::jetson_to_sec::Payload::SeRequest(
                    security_messaging::SeRequest {
                        id: security_messaging::se_request::RequestType::PowerOff
                            as u32,
                        data: vec![],
                        rx_length: 0,
                        request_type: 0,
                    },
                ),
            ))
            .await
        {
            Ok(CommonAckError::Success) => {
                info!("🔌 Power cycling secure element");
            }
            Ok(ack) => {
                error!("Failed to power cycle secure element: ack: {:?}", ack);
            }
            Err(e) => {
                error!("Failed to power cycle secure element: {:?}", e);
            }
        }
        Ok(())
    }
}

#[async_trait]
impl Board for SecurityBoard {
    async fn reboot(&mut self, delay: Option<u32>) -> Result<()> {
        let delay = delay.unwrap_or(REBOOT_DELAY);
        let reboot_msg =
            McuPayload::ToSec(orb_messages::sec::jetson_to_sec::Payload::Reboot(
                orb_messages::RebootWithDelay { delay },
            ));
        match self.send(reboot_msg).await {
            Ok(CommonAckError::Success) => {
                info!("🚦 Rebooting security microcontroller in {} seconds", delay);
            }
            Ok(ack) => {
                error!("Failed to reboot security microcontroller: ack: {:?}", ack);
            }
            Err(e) => {
                error!("Failed to reboot security microcontroller: {:?}", e);
            }
        }
        Ok(())
    }

    async fn fetch_info(&mut self, info: &mut OrbInfo) -> Result<()> {
        let board_info = SecurityBoardInfo::new()
            .build(self)
            .await
            .unwrap_or_else(|board_info| board_info);

        info.sec_fw_versions = board_info.fw_versions;
        info.sec_battery_status = board_info.battery_status;

        Ok(())
    }

    async fn dump(
        &mut self,
        duration: Option<Duration>,
        logs_only: bool,
    ) -> Result<()> {
        let until_time = duration.map(|d| std::time::Instant::now() + d);

        loop {
            if let Some(until_time) = until_time {
                if std::time::Instant::now() > until_time {
                    break;
                }
            }

            while let Ok(McuPayload::FromSec(sec_mcu_payload)) =
                self.message_queue_rx.try_recv()
            {
                if logs_only {
                    if let security_messaging::sec_to_jetson::Payload::Log(log) =
                        sec_mcu_payload
                    {
                        println!("{}", log.log);
                    }
                } else {
                    println!("{:?}", sec_mcu_payload);
                }
            }

            time::sleep(Duration::from_millis(200)).await;
        }
        Ok(())
    }

    async fn update_firmware(&mut self, path: &str) -> Result<()> {
        let buffer = dfu::load_binary_file(path)?;
        debug!("Sending file {} ({} bytes)", path, buffer.len());
        let mut block_iter =
            BlockIterator::<security_messaging::jetson_to_sec::Payload>::new(
                buffer.as_slice(),
            );

        while let Some(payload) = block_iter.next() {
            while self.send(McuPayload::ToSec(payload.clone())).await.is_err() {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            dfu::print_progress(block_iter.progress_percentage());
        }
        dfu::print_progress(100.0);
        println!();

        // check CRC32 of sent firmware image
        let crc = crc32fast::hash(buffer.as_slice());
        let payload = McuPayload::ToSec(
            security_messaging::jetson_to_sec::Payload::FwImageCheck(
                orb_messages::FirmwareImageCheck { crc32: crc },
            ),
        );
        if let Ok(ack) = self.send(payload).await {
            if !matches!(ack, CommonAckError::Success) {
                return Err(eyre!(
                    "Unable to check image integrity: ack error: {}",
                    ack as i32
                ));
            }
            info!("✅ Image integrity confirmed, activating image");
        } else {
            return Err(eyre!("Firmware image integrity check failed"));
        }

        self.switch_images().await?;

        info!("👉 Rebooting the security microcontroller to install the new image");
        self.reboot(Some(3)).await?;

        Ok(())
    }

    async fn switch_images(&mut self) -> Result<()> {
        let board_info = SecurityBoardInfo::new()
            .build(self)
            .await
            .unwrap_or_else(|board_info| board_info);
        if let Some(fw_versions) = board_info.fw_versions {
            if let Some(secondary_app) = fw_versions.secondary_app {
                if let Some(primary_app) = fw_versions.primary_app {
                    return if (primary_app.commit_hash == 0
                        && secondary_app.commit_hash != 0)
                        || (primary_app.commit_hash != 0
                            && secondary_app.commit_hash == 0)
                    {
                        Err(eyre!("Primary and secondary images types (prod or dev) don't match"))
                    } else {
                        let payload = McuPayload::ToSec(
                            security_messaging::jetson_to_sec::Payload::FwImageSecondaryActivate(
                                orb_messages::FirmwareActivateSecondary {
                                    force_permanent: false,
                                },
                            ),
                        );
                        if let Ok(ack) = self.send(payload).await {
                            if !matches!(ack, CommonAckError::Success) {
                                return Err(eyre!(
                                    "Unable to activate image: ack error: {}",
                                    ack as i32
                                ));
                            }
                        }
                        info!("✅ Image activated for installation after reboot");
                        Ok(())
                    };
                }
            }
        }

        Err(eyre!("Firmware versions can't be verified"))
    }

    async fn stress_test(&mut self, duration: Option<Duration>) -> Result<()> {
        let test_count = 2;
        let mut test_idx = 0;
        let mut success_count = 0;
        let mut error_count = 0;
        while test_idx < test_count {
            let starting_time = std::time::Instant::now();
            let until_time = if let Some(duration) = duration {
                std::time::Instant::now() + duration / test_count
            } else {
                std::time::Instant::now() + Duration::from_secs(3)
            };

            loop {
                if std::time::Instant::now() > until_time {
                    break;
                }

                let payload = McuPayload::ToSec(
                    security_messaging::jetson_to_sec::Payload::ValueGet(
                        orb_messages::ValueGet {
                            value: orb_messages::value_get::Value::FirmwareVersions
                                as i32,
                        },
                    ),
                );

                let res = match test_idx {
                    0 => self.isotp_iface.send(payload).await,
                    1 => self.canfd_iface.send(payload).await,
                    _ => {
                        // todo serial
                        panic!("Serial stress test not implemented");
                    }
                };

                if let Ok(ack) = res {
                    if matches!(ack, CommonAckError::Success) {
                        success_count += 1;
                    } else {
                        error_count += 1;
                    }
                } else {
                    error_count += 1;
                }
            }

            let tx_count = success_count + error_count;
            info!(
                "📈 {} #{:8}\t⚡️ {:4} v/s\t\t✅ {:}%\t\t❌ {:}%\t[{}]",
                if test_idx == 0 { "ISO-TP" } else { "CAN-FD" },
                tx_count,
                tx_count * 1000 / (starting_time.elapsed().as_millis() as u32),
                success_count * 100 / tx_count,
                100 - (success_count * 100 / tx_count),
                std::process::id()
            );

            // reset counters and move to the next test
            success_count = 0;
            error_count = 0;
            test_idx += 1;
            if duration.is_none() {
                test_idx %= test_count;
            }
        }

        Ok(())
    }
}

struct SecurityBoardInfo {
    fw_versions: Option<orb_messages::Versions>,
    battery_status: Option<BatteryStatus>,
}

impl SecurityBoardInfo {
    fn new() -> Self {
        Self {
            fw_versions: None,
            battery_status: None,
        }
    }

    /// Fetches `SecurityBoardInfo` from the security board
    /// on timeout, returns the info that was fetched so far
    async fn build(mut self, sec_board: &mut SecurityBoard) -> Result<Self, Self> {
        let mut is_err = false;
        match sec_board
            .send(McuPayload::ToSec(
                security_messaging::jetson_to_sec::Payload::ValueGet(
                    orb_messages::ValueGet {
                        value: orb_messages::value_get::Value::FirmwareVersions as i32,
                    },
                ),
            ))
            .await
        {
            Ok(CommonAckError::Success) => { /* nothing */ }
            Ok(a) => {
                is_err = true;
                error!("error asking for firmware version: {a:?}");
            }
            Err(e) => {
                is_err = true;
                error!("error asking for firmware version: {e:?}");
            }
        }

        match sec_board
            .send(McuPayload::ToSec(
                security_messaging::jetson_to_sec::Payload::ValueGet(
                    orb_messages::ValueGet {
                        value: orb_messages::value_get::Value::HardwareVersions as i32,
                    },
                ),
            ))
            .await
        {
            Ok(CommonAckError::Success) => { /* nothing */ }
            Ok(a) => {
                is_err = true;
                error!("error asking for hardware version: {a:?}");
            }
            Err(e) => {
                is_err = true;
                error!("error asking for hardware version: {e:?}");
            }
        }

        match sec_board
            .send(McuPayload::ToSec(
                security_messaging::jetson_to_sec::Payload::ValueGet(
                    orb_messages::ValueGet {
                        value: orb_messages::value_get::Value::BatteryStatus as i32,
                    },
                ),
            ))
            .await
        {
            Ok(CommonAckError::Success) => { /* nothing */ }
            Ok(a) => {
                is_err = true;
                error!("error asking for battery status: {a:?}");
            }
            Err(e) => {
                is_err = true;
                error!("error asking for battery status: {e:?}");
            }
        }

        match tokio::time::timeout(
            Duration::from_secs(2),
            self.listen_for_board_info(sec_board),
        )
        .await
        {
            Err(tokio::time::error::Elapsed { .. }) => {
                warn!("Timeout waiting on security board info");
                is_err = true;
            }
            Ok(()) => {
                debug!("Got security board info");
            }
        }

        if is_err {
            Ok(self)
        } else {
            Err(self)
        }
    }

    /// Mutates `self` while listening for board info messages.
    ///
    /// Does not terminate until all board info is populated.
    async fn listen_for_board_info(&mut self, sec_board: &mut SecurityBoard) {
        let mut battery_status = BatteryStatus {
            percentage: None,
            voltage_mv: None,
            is_charging: None,
        };
        loop {
            let Some(mcu_payload) = sec_board.message_queue_rx.recv().await else {
                warn!("security board queue is closed");
                return;
            };
            let McuPayload::FromSec(sec_mcu_payload) = mcu_payload else {
                unreachable!("should always be a message from the security board")
            };

            tracing::trace!("rx message from sec-mcu: {:?}", sec_mcu_payload);
            match sec_mcu_payload {
                security_messaging::sec_to_jetson::Payload::Versions(v) => {
                    self.fw_versions = Some(v);
                }
                security_messaging::sec_to_jetson::Payload::BatteryStatus(b) => {
                    battery_status.percentage = Some(b.percentage as u32);
                    battery_status.voltage_mv = Some(b.voltage_mv as u32);
                    battery_status.is_charging =
                        Some(b.state == (BatteryState::Charging as i32));
                }
                _ => {}
            }

            if self.battery_status.is_none()
                && battery_status.voltage_mv.is_some()
                && battery_status.percentage.is_some()
                && battery_status.is_charging.is_some()
            {
                self.battery_status = Some(battery_status.clone());
            }

            // check that all fields are set in BoardInfo
            if self.fw_versions.is_some() && self.battery_status.is_some() {
                return;
            }
        }
    }
}
