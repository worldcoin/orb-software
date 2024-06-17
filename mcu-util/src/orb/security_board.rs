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
use orb_messages::mcu_sec::battery_status::BatteryState;
use orb_messages::{mcu_sec as security_messaging, CommonAckError};

use crate::orb::dfu::BlockIterator;
use crate::orb::{dfu, BatteryStatus};
use crate::orb::{Board, OrbInfo};

use super::BoardTaskHandles;

const REBOOT_DELAY: u32 = 3;

pub struct SecurityBoard {
    canfd_iface: CanRawMessaging,
    isotp_iface: CanIsoTpMessaging,
    message_queue_rx: mpsc::UnboundedReceiver<McuPayload>,
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

    pub async fn build(self) -> Result<(SecurityBoard, BoardTaskHandles)> {
        let (mut canfd_iface, raw_can_task) = CanRawMessaging::new(
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

        // Send a heartbeat to the mcu to ensure it is alive
        // & "subscribe" to the mcu messages: messages to the Jetson
        // are going to be sent after the heartbeat
        let ack_result = canfd_iface
            .send(McuPayload::ToSec(
                security_messaging::jetson_to_sec::Payload::Heartbeat(
                    security_messaging::Heartbeat {
                        timeout_seconds: 0_u32,
                    },
                ),
            ))
            .await
            .map(|c| {
                if let CommonAckError::Success = c {
                    Ok(())
                } else {
                    Err(eyre!("ack error: {c}"))
                }
            });
        if let Err(e) = ack_result {
            error!("Failed to send heartbeat to security mcu: {:#?}", e);
        }

        Ok((
            SecurityBoard {
                canfd_iface,
                isotp_iface,
                message_queue_rx: self.message_queue_rx,
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

    pub async fn power_cycle_secure_element(&mut self) -> Result<()> {
        self.isotp_iface
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
            .await?;
        info!("🔌 Power cycling secure element");
        Ok(())
    }
}

#[async_trait]
impl Board for SecurityBoard {
    async fn reboot(&mut self, delay: Option<u32>) -> Result<()> {
        let delay = delay.unwrap_or(REBOOT_DELAY);
        self.isotp_iface
            .send(McuPayload::ToSec(
                orb_messages::mcu_sec::jetson_to_sec::Payload::Reboot(
                    security_messaging::RebootWithDelay { delay },
                ),
            ))
            .await?;
        info!("🚦 Rebooting security microcontroller in {} seconds", delay);
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

    async fn update_firmware(&mut self, path: &str, canfd: bool) -> Result<()> {
        let buffer = dfu::load_binary_file(path)?;
        debug!("Sending file {} ({} bytes)", path, buffer.len());
        let mut block_iter =
            BlockIterator::<security_messaging::jetson_to_sec::Payload>::new(
                buffer.as_slice(),
            );

        while let Some(payload) = block_iter.next() {
            if canfd {
                while self
                    .canfd_iface
                    .send(McuPayload::ToSec(payload.clone()))
                    .await
                    .is_err()
                {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            } else {
                while self
                    .isotp_iface
                    .send(McuPayload::ToSec(payload.clone()))
                    .await
                    .is_err()
                {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
            dfu::print_progress(block_iter.progress_percentage());
        }
        dfu::print_progress(100.0);
        println!();

        // check CRC32 of sent firmware image
        let crc = crc32fast::hash(buffer.as_slice());
        let payload = McuPayload::ToSec(
            security_messaging::jetson_to_sec::Payload::FwImageCheck(
                security_messaging::FirmwareImageCheck { crc32: crc },
            ),
        );
        if let Ok(ack) = self.isotp_iface.send(payload).await {
            if !matches!(ack, CommonAckError::Success) {
                return Err(eyre!(
                    "Unable to validate image: ack error: {}",
                    ack as i32
                ));
            }
            info!("✅ Image integrity confirmed, activating image");
        } else {
            return Err(eyre!("Firmware image check failed"));
        }

        self.switch_images().await?;

        info!("👉 Shut the Orb down to install the new image");
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
                                security_messaging::FirmwareActivateSecondary {
                                    force_permanent: false,
                                },
                            ),
                        );
                        if let Ok(ack) = self.isotp_iface.send(payload).await {
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
                        security_messaging::ValueGet {
                            value:
                                security_messaging::value_get::Value::FirmwareVersions
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
    fw_versions: Option<security_messaging::Versions>,
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
        if let Err(e) = sec_board
            .isotp_iface
            .send(McuPayload::ToSec(
                security_messaging::jetson_to_sec::Payload::ValueGet(
                    security_messaging::ValueGet {
                        value: security_messaging::value_get::Value::FirmwareVersions
                            as i32,
                    },
                ),
            ))
            .await
        {
            is_err = true;
            error!("Failed to fetch firmware versions: {:#?}", e);
        }

        if let Err(e) = sec_board
            .isotp_iface
            .send(McuPayload::ToSec(
                security_messaging::jetson_to_sec::Payload::ValueGet(
                    security_messaging::ValueGet {
                        value: security_messaging::value_get::Value::HardwareVersions
                            as i32,
                    },
                ),
            ))
            .await
        {
            is_err = true;
            error!("Failed to fetch hardware versions: {:#?}", e);
        }

        if let Err(e) = sec_board
            .isotp_iface
            .send(McuPayload::ToSec(
                security_messaging::jetson_to_sec::Payload::ValueGet(
                    security_messaging::ValueGet {
                        value: security_messaging::value_get::Value::BatteryStatus
                            as i32,
                    },
                ),
            ))
            .await
        {
            is_err = true;
            error!("Failed to fetch battery status: {:#?}", e);
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
