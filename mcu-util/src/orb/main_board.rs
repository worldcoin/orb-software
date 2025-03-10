use async_trait::async_trait;
use color_eyre::eyre::{eyre, Result, WrapErr as _};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time;
use tracing::{debug, error, info, warn};

use crate::orb::dfu::BlockIterator;
use crate::orb::revision::OrbRevision;
use crate::orb::{dfu, BatteryStatus};
use crate::orb::{Board, OrbInfo};
use crate::{Camera, Leds};
use orb_mcu_interface::can::canfd::CanRawMessaging;
use orb_mcu_interface::can::isotp::{CanIsoTpMessaging, IsoTpNodeIdentifier};
use orb_mcu_interface::orb_messages;
use orb_mcu_interface::orb_messages::{main as main_messaging, CommonAckError};
use orb_mcu_interface::{Device, McuPayload, MessagingInterface};

use super::BoardTaskHandles;

const REBOOT_DELAY: u32 = 3;

pub struct MainBoard {
    canfd_iface: CanRawMessaging,
    isotp_iface: CanIsoTpMessaging,
    message_queue_rx: mpsc::UnboundedReceiver<McuPayload>,
    canfd: bool,
}

pub struct MainBoardBuilder {
    message_queue_rx: mpsc::UnboundedReceiver<McuPayload>,
    message_queue_tx: mpsc::UnboundedSender<McuPayload>,
}

impl MainBoardBuilder {
    pub(crate) fn new() -> Self {
        let (message_queue_tx, message_queue_rx) =
            mpsc::unbounded_channel::<McuPayload>();

        Self {
            message_queue_rx,
            message_queue_tx,
        }
    }

    pub async fn build(self, canfd: bool) -> Result<(MainBoard, BoardTaskHandles)> {
        let (canfd_iface, raw_can_task_handle) = CanRawMessaging::new(
            String::from("can0"),
            Device::Main,
            self.message_queue_tx.clone(),
        )
        .wrap_err("Failed to create CanRawMessaging for MainBoard")?;

        let (isotp_iface, isotp_can_task_handle) = CanIsoTpMessaging::new(
            String::from("can0"),
            IsoTpNodeIdentifier::JetsonApp7,
            IsoTpNodeIdentifier::MainMcu,
            self.message_queue_tx.clone(),
        )
        .wrap_err("Failed to create CanIsoTpMessaging for MainBoard")?;

        Ok((
            MainBoard {
                canfd_iface,
                isotp_iface,
                message_queue_rx: self.message_queue_rx,
                canfd,
            },
            BoardTaskHandles {
                raw: raw_can_task_handle,
                isotp: isotp_can_task_handle,
            },
        ))
    }
}

impl MainBoard {
    pub fn builder() -> MainBoardBuilder {
        MainBoardBuilder::new()
    }

    /// Send a message to the security board with preferred interface
    pub async fn send(&mut self, payload: McuPayload) -> Result<CommonAckError> {
        if matches!(payload, McuPayload::ToMain(_)) {
            tracing::trace!(
                "sending to main mcu over {}: {:?}",
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

    pub async fn gimbal_auto_home(&mut self) -> Result<()> {
        match self
            .isotp_iface
            .send(McuPayload::ToMain(
                main_messaging::jetson_to_mcu::Payload::DoHoming(
                    main_messaging::PerformMirrorHoming {
                        homing_mode:
                            main_messaging::perform_mirror_homing::Mode::OneBlockingEnd
                                as i32,
                        angle: main_messaging::perform_mirror_homing::Angle::Both
                            as i32,
                    },
                ),
            ))
            .await?
        {
            CommonAckError::Success => {
                info!("✅ Gimbal went back home");
                Ok(())
            }
            ack_err => Err(eyre!("Gimbal auto home failed: ack error: {ack_err}")),
        }
    }

    pub async fn gimbal_set_position(
        &mut self,
        phi_angle_millidegrees: u32,
        theta_angle_millidegrees: u32,
    ) -> Result<()> {
        match self
            .isotp_iface
            .send(McuPayload::ToMain(
                main_messaging::jetson_to_mcu::Payload::MirrorAngle(
                    main_messaging::MirrorAngle {
                        horizontal_angle: 0,
                        vertical_angle: 0,
                        angle_type: main_messaging::MirrorAngleType::PhiTheta as i32,
                        phi_angle_millidegrees,
                        theta_angle_millidegrees,
                    },
                ),
            ))
            .await?
        {
            CommonAckError::Success => {
                info!("✅ Gimbal position set");
                Ok(())
            }
            ack_err => Err(eyre!("Gimbal set position failed: ack error: {ack_err}")),
        }
    }

    pub async fn trigger_camera(&mut self, cam: Camera) -> Result<()> {
        // set FPS to 30
        match self
            .isotp_iface
            .send(McuPayload::ToMain(
                main_messaging::jetson_to_mcu::Payload::Fps(main_messaging::Fps {
                    fps: 30,
                }),
            ))
            .await
        {
            Ok(_) => {
                info!("🎥 FPS set to 30");
            }
            Err(e) => {
                return Err(eyre!("Error setting FPS: {:?}", e));
            }
        }

        // set on-duration to 300us
        match self
            .isotp_iface
            .send(McuPayload::ToMain(
                main_messaging::jetson_to_mcu::Payload::LedOnTime(
                    main_messaging::LedOnTimeUs {
                        on_duration_us: 300,
                    },
                ),
            ))
            .await
        {
            Ok(_) => {
                info!("💡 LED on duration set to 300us");
            }
            Err(e) => {
                return Err(eyre!("Error setting on-duration: {:?}", e));
            }
        }

        // enable wavelength 850nm
        match self.isotp_iface.send(McuPayload::ToMain(
            main_messaging::jetson_to_mcu::Payload::InfraredLeds(main_messaging::InfraredLeDs {
                wavelength: orb_messages::main::infrared_le_ds::Wavelength::Wavelength850nm as i32,
            }))).await {
            Ok(_) => {
                info!("⚡️ 850nm infrared LEDs enabled");
            }
            Err(e) => {
                return Err(eyre!("Error enabling infrared leds: {:?}", e));
            }
        }

        // enable camera trigger
        match cam {
            Camera::Eye => {
                match self.isotp_iface.send(McuPayload::ToMain(
                    main_messaging::jetson_to_mcu::Payload::StartTriggeringIrEyeCamera(main_messaging::StartTriggeringIrEyeCamera {}))).await {
                    Ok(_) => {
                        info!("📸 Eye camera trigger enabled");
                    }
                    Err(e) => {
                        return Err(eyre!("Error enabling eye camera trigger: {:?}", e));
                    }
                }
            }
            Camera::Face => {
                match self.isotp_iface.send(McuPayload::ToMain(
                    main_messaging::jetson_to_mcu::Payload::StartTriggeringIrFaceCamera(main_messaging::StartTriggeringIrFaceCamera {}))).await {
                    Ok(_) => {
                        info!("📸 Face camera trigger enabled");
                    }
                    Err(e) => {
                        return Err(eyre!("Error enabling eye camera trigger: {:?}", e));
                    }
                }
            }
        }

        Ok(())
    }

    pub async fn front_leds(&mut self, leds: Leds) -> Result<()> {
        if let Leds::Booster = leds {
            match self
                .isotp_iface
                .send(McuPayload::ToMain(
                    main_messaging::jetson_to_mcu::Payload::WhiteLedsBrightness(
                        main_messaging::WhiteLeDsBrightness { brightness: 5 },
                    ),
                ))
                .await
            {
                Ok(_) => {
                    info!("🚀 Booster LEDs enabled");
                }
                Err(e) => {
                    return Err(eyre!("Error enabling booster LEDs: {:?}", e));
                }
            }
        } else {
            let pattern = match leds {
                Leds::Red => {
                    main_messaging::user_le_ds_pattern::UserRgbLedPattern::AllRed
                }
                Leds::Green => {
                    main_messaging::user_le_ds_pattern::UserRgbLedPattern::AllGreen
                }
                Leds::Blue => {
                    main_messaging::user_le_ds_pattern::UserRgbLedPattern::AllBlue
                }
                Leds::White => {
                    main_messaging::user_le_ds_pattern::UserRgbLedPattern::AllWhite
                }
                _ => {
                    error!("Invalid rgb color");
                    return Err(eyre!("Invalid LEDs"));
                }
            };

            match self
                .isotp_iface
                .send(McuPayload::ToMain(
                    main_messaging::jetson_to_mcu::Payload::UserLedsPattern(
                        main_messaging::UserLeDsPattern {
                            pattern: pattern as i32,
                            custom_color: None,
                            start_angle: 0,
                            angle_length: 360,
                            pulsing_scale: 0.0,
                            pulsing_period_ms: 0,
                        },
                    ),
                ))
                .await
            {
                Ok(_) => {
                    info!("🚦 {:?} enabled", pattern);
                }
                Err(e) => {
                    return Err(eyre!("Error enabling green LEDs: {:?}", e));
                }
            }
        }

        // turn off all LEDs after 3 seconds
        tokio::time::sleep(Duration::from_millis(3000)).await;

        if let Leds::Booster = leds {
            match self
                .isotp_iface
                .send(McuPayload::ToMain(
                    main_messaging::jetson_to_mcu::Payload::WhiteLedsBrightness(
                        main_messaging::WhiteLeDsBrightness { brightness: 0 },
                    ),
                ))
                .await
            {
                Ok(_) => {
                    info!("LEDs disabled");
                }
                Err(e) => {
                    return Err(eyre!("Error disabling booster LEDs: {:?}", e));
                }
            }
        } else {
            match self
                .isotp_iface
                .send(McuPayload::ToMain(
                    main_messaging::jetson_to_mcu::Payload::UserLedsPattern(
                        main_messaging::UserLeDsPattern {
                            pattern:
                            main_messaging::user_le_ds_pattern::UserRgbLedPattern::Off
                                as i32,
                            custom_color: None,
                            start_angle: 0,
                            angle_length: 360,
                            pulsing_scale: 0.0,
                            pulsing_period_ms: 0,
                        },
                    ),
                ))
                .await
            {
                Ok(_) => {
                    info!("LEDs disabled");
                }
                Err(e) => {
                    return Err(eyre!("Error disabling RGB LEDs: {:?}", e));
                }
            }
        }

        Ok(())
    }
}

#[async_trait]
impl Board for MainBoard {
    async fn reboot(&mut self, delay: Option<u32>) -> Result<()> {
        let delay = delay.unwrap_or(REBOOT_DELAY);
        let reboot_msg =
            McuPayload::ToMain(main_messaging::jetson_to_mcu::Payload::Reboot(
                orb_messages::RebootWithDelay { delay },
            ));
        self.send(reboot_msg).await?;
        info!("🚦 Rebooting main microcontroller in {} seconds", delay);
        Ok(())
    }

    async fn fetch_info(&mut self, info: &mut OrbInfo) -> Result<()> {
        let board_info = MainBoardInfo::new()
            .build(self)
            .await
            .unwrap_or_else(|board_info| board_info);

        info.hw_rev = board_info.hw_version;
        info.main_fw_versions = board_info.fw_versions;
        info.main_battery_status = board_info.battery_status;

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

            while let Ok(McuPayload::FromMain(main_mcu_payload)) =
                self.message_queue_rx.try_recv()
            {
                if logs_only {
                    if let main_messaging::mcu_to_jetson::Payload::Log(log) =
                        main_mcu_payload
                    {
                        println!("{}", log.log);
                    }
                } else {
                    println!("{:?}", main_mcu_payload);
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
            BlockIterator::<main_messaging::jetson_to_mcu::Payload>::new(
                buffer.as_slice(),
            );

        while let Some(payload) = block_iter.next() {
            while self
                .send(McuPayload::ToMain(payload.clone()))
                .await
                .is_err()
            {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            dfu::print_progress(block_iter.progress_percentage());
        }
        dfu::print_progress(100.0);
        println!();

        // check CRC32 of sent firmware image
        let crc = crc32fast::hash(buffer.as_slice());
        let payload =
            McuPayload::ToMain(main_messaging::jetson_to_mcu::Payload::FwImageCheck(
                orb_messages::FirmwareImageCheck { crc32: crc },
            ));

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

        info!("👉 Shut the Orb down to install the new image (`sudo shutdown now`), the Orb is going to reboot itself once installation is complete");
        Ok(())
    }

    async fn switch_images(&mut self) -> Result<()> {
        let board_info = MainBoardInfo::new()
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
                        let payload = McuPayload::ToMain(
                            main_messaging::jetson_to_mcu::Payload::FwImageSecondaryActivate(
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
                        info!("✅ Image activated for installation after reboot (use `sudo shutdown now` to gracefully install the image)");
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

                let payload = McuPayload::ToMain(
                    main_messaging::jetson_to_mcu::Payload::ValueGet(
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

struct MainBoardInfo {
    hw_version: Option<OrbRevision>,
    fw_versions: Option<orb_messages::Versions>,
    battery_status: Option<BatteryStatus>,
}

impl MainBoardInfo {
    fn new() -> Self {
        Self {
            hw_version: None,
            fw_versions: None,
            battery_status: None,
        }
    }

    /// Fetches `MainBoardInfo` from the main board
    /// doesn't fail, but lazily fetches as much info as it could
    /// on timeout, returns the info that was fetched so far
    async fn build(mut self, main_board: &mut MainBoard) -> Result<Self, Self> {
        let mut is_err = false;

        if let Err(e) = main_board
            .send(McuPayload::ToMain(
                main_messaging::jetson_to_mcu::Payload::ValueGet(
                    orb_messages::ValueGet {
                        value: orb_messages::value_get::Value::FirmwareVersions as i32,
                    },
                ),
            ))
            .await
        {
            is_err = true;
            error!("error asking for firmware version: {e}");
        }

        if let Err(e) = main_board
            .send(McuPayload::ToMain(
                main_messaging::jetson_to_mcu::Payload::ValueGet(
                    orb_messages::ValueGet {
                        value: orb_messages::value_get::Value::HardwareVersions as i32,
                    },
                ),
            ))
            .await
        {
            is_err = true;
            error!("error asking for hardware version: {e}");
        }

        if let Err(e) = main_board
            .send(McuPayload::ToMain(
                main_messaging::jetson_to_mcu::Payload::ValueGet(
                    orb_messages::ValueGet {
                        value: orb_messages::value_get::Value::BatteryStatus as i32,
                    },
                ),
            ))
            .await
        {
            is_err = true;
            error!("error asking for battery status: {e}");
        }

        match tokio::time::timeout(
            Duration::from_secs(2),
            self.listen_for_board_info(main_board),
        )
        .await
        {
            Err(tokio::time::error::Elapsed { .. }) => {
                warn!("Timeout waiting on main board info");
                is_err = true;
            }
            Ok(()) => {
                debug!("Got main board info");
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
    async fn listen_for_board_info(&mut self, main_board: &mut MainBoard) {
        let mut battery_status = BatteryStatus {
            percentage: None,
            voltage_mv: None,
            is_charging: None,
        };

        loop {
            let Some(mcu_payload) = main_board.message_queue_rx.recv().await else {
                warn!("main board queue is closed");
                return;
            };
            let McuPayload::FromMain(main_mcu_payload) = mcu_payload else {
                unreachable!("should always be a message from the main board")
            };

            tracing::trace!("rx message from main-mcu: {:?}", main_mcu_payload);
            match main_mcu_payload {
                main_messaging::mcu_to_jetson::Payload::Versions(v) => {
                    self.fw_versions = Some(v);
                }
                main_messaging::mcu_to_jetson::Payload::Hardware(h) => {
                    self.hw_version = Some(OrbRevision(h));
                }
                main_messaging::mcu_to_jetson::Payload::BatteryCapacity(b) => {
                    battery_status.percentage = Some(b.percentage);
                }
                main_messaging::mcu_to_jetson::Payload::BatteryVoltage(b) => {
                    battery_status.voltage_mv = Some(
                        (b.battery_cell1_mv
                            + b.battery_cell2_mv
                            + b.battery_cell3_mv
                            + b.battery_cell4_mv) as u32,
                    );
                }
                main_messaging::mcu_to_jetson::Payload::BatteryIsCharging(b) => {
                    battery_status.is_charging = Some(b.battery_is_charging);
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
            if self.hw_version.is_some()
                && self.fw_versions.is_some()
                && self.battery_status.is_some()
            {
                return;
            }
        }
    }
}
