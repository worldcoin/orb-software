#![allow(clippy::uninlined_format_args)]
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
use crate::{Camera, GimbalHomeOpts, Leds, PolarizerOpts};
use orb_mcu_interface::can::canfd::CanRawMessaging;
use orb_mcu_interface::can::isotp::{CanIsoTpMessaging, IsoTpNodeIdentifier};
use orb_mcu_interface::orb_messages;
use orb_mcu_interface::orb_messages::{main as main_messaging, CommonAckError};
use orb_mcu_interface::{Device, McuPayload, MessagingInterface};

use super::BoardTaskHandles;

const REBOOT_DELAY: u32 = 3;

pub struct MainBoard {
    canfd_iface: CanRawMessaging,
    isotp_iface: Option<CanIsoTpMessaging>,
    message_queue_rx: mpsc::UnboundedReceiver<McuPayload>,
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

        // Only create ISO-TP interface when **not** using CAN-FD
        // on user's demand (--can-fd flag)
        // the isotp kernel module might not be inserted and thus
        // would cause errors if we try to use it
        let (isotp_iface, isotp_can_task_handle) = if canfd {
            (None, None)
        } else {
            let (iface, task) = CanIsoTpMessaging::new(
                String::from("can0"),
                IsoTpNodeIdentifier::JetsonApp7,
                IsoTpNodeIdentifier::MainMcu,
                self.message_queue_tx.clone(),
            )
            .wrap_err("Failed to create CanIsoTpMessaging for MainBoard")?;
            (Some(iface), Some(task))
        };

        Ok((
            MainBoard {
                canfd_iface,
                isotp_iface,
                message_queue_rx: self.message_queue_rx,
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

    /// Send a message to the main board with preferred interface
    pub async fn send(&mut self, payload: McuPayload) -> Result<CommonAckError> {
        if matches!(payload, McuPayload::ToMain(_)) {
            if let Some(isotp_iface) = &mut self.isotp_iface {
                tracing::trace!("sending to main mcu over iso-tp: {:?}", payload);
                isotp_iface.send(payload).await
            } else {
                tracing::trace!("sending to main mcu over can-fd: {:?}", payload);
                self.canfd_iface.send(payload).await
            }
        } else {
            Err(eyre!("Message not targeted to main board: {:?}", payload))
        }
    }

    pub async fn gimbal_auto_home(&mut self, home_opts: GimbalHomeOpts) -> Result<()> {
        let homing_mode = match home_opts {
            GimbalHomeOpts::Autohome => {
                main_messaging::perform_mirror_homing::Mode::OneBlockingEnd as i32
            }
            GimbalHomeOpts::ShortestPath => {
                main_messaging::perform_mirror_homing::Mode::WithKnownCoordinates as i32
            }
        };

        match self
            .send(McuPayload::ToMain(
                main_messaging::jetson_to_mcu::Payload::DoHoming(
                    main_messaging::PerformMirrorHoming {
                        homing_mode,
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

    pub(crate) async fn gimbal_move(
        &mut self,
        phi_angle_millidegrees: i32,
        theta_angle_millidegrees: i32,
    ) -> Result<()> {
        match self
            .send(McuPayload::ToMain(
                main_messaging::jetson_to_mcu::Payload::MirrorAngleRelative(
                    main_messaging::MirrorAngleRelative {
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
                info!("✅ Gimbal moved");
                Ok(())
            }
            ack_err => Err(eyre!("Gimbal move position failed: ack error: {ack_err}")),
        }
    }

    pub async fn trigger_camera(&mut self, cam: Camera, fps: u32) -> Result<()> {
        // set FPS to provided value
        match self
            .send(McuPayload::ToMain(
                main_messaging::jetson_to_mcu::Payload::Fps(main_messaging::Fps {
                    fps,
                }),
            ))
            .await
        {
            Ok(CommonAckError::Success) => {
                info!("🎥 FPS set to {}", fps);
            }
            Ok(ack_err) => {
                return Err(eyre!("Error setting FPS: ack: {:?}", ack_err));
            }
            Err(e) => {
                return Err(eyre!("Error setting FPS: {:?}", e));
            }
        }

        // set on-duration to 300us
        match self
            .send(McuPayload::ToMain(
                main_messaging::jetson_to_mcu::Payload::LedOnTime(
                    main_messaging::LedOnTimeUs {
                        on_duration_us: 300,
                    },
                ),
            ))
            .await
        {
            Ok(CommonAckError::Success) => {
                info!("💡 LED on duration set to 300us");
            }
            Ok(ack_err) => {
                return Err(eyre!("Error setting on-duration: ack: {:?}", ack_err));
            }
            Err(e) => {
                return Err(eyre!("Error setting on-duration: {:?}", e));
            }
        }

        // enable wavelength 850nm
        match self.send(McuPayload::ToMain(
            main_messaging::jetson_to_mcu::Payload::InfraredLeds(main_messaging::InfraredLeDs {
                wavelength: orb_messages::main::infrared_le_ds::Wavelength::Wavelength850nm as i32,
            }))).await {
            Ok(CommonAckError::Success) => {
                info!("⚡️ 850nm infrared LEDs enabled");
            }
            Ok(ack_err) => {
                return Err(eyre!("Error enabling infrared leds: ack: {:?}", ack_err));
            }
            Err(e) => {
                return Err(eyre!("Error enabling infrared leds: {:?}", e));
            }
        }

        // enable camera trigger
        match cam {
            Camera::Eye { .. } => {
                match self.send(McuPayload::ToMain(
                    main_messaging::jetson_to_mcu::Payload::StartTriggeringIrEyeCamera(main_messaging::StartTriggeringIrEyeCamera {}))).await {
                    Ok(CommonAckError::Success) => {
                        info!("📸 Eye camera trigger enabled");
                    }
                    Ok(e) => {
                        return Err(eyre!("Error enabling eye camera trigger: ack {:?}", e));
                    }
                    Err(e) => {
                        return Err(eyre!("Error enabling eye camera trigger: {:?}", e));
                    }
                }
            }
            Camera::Face { .. } => {
                match self.send(McuPayload::ToMain(
                    main_messaging::jetson_to_mcu::Payload::StartTriggeringIrFaceCamera(main_messaging::StartTriggeringIrFaceCamera {}))).await {
                    Ok(CommonAckError::Success) => {
                        info!("📸 Face camera trigger enabled");
                    }
                    Ok(e) => {
                        return Err(eyre!("Error enabling face camera trigger: ack {:?}", e));
                    }
                    Err(e) => {
                        return Err(eyre!("Error enabling eye camera trigger: {:?}", e));
                    }
                }
            }
        }

        Ok(())
    }

    pub(crate) async fn polarizer(&mut self, opts: PolarizerOpts) -> Result<()> {
        let (command, angle) = match opts {
            PolarizerOpts::Home => (
                main_messaging::polarizer::Command::PolarizerHome as i32,
                None,
            ),
            PolarizerOpts::Passthrough => (
                main_messaging::polarizer::Command::PolarizerPassThrough as i32,
                None,
            ),
            PolarizerOpts::Horizontal => (
                main_messaging::polarizer::Command::Polarizer0Horizontal as i32,
                None,
            ),
            PolarizerOpts::Vertical => (
                main_messaging::polarizer::Command::Polarizer90Vertical as i32,
                None,
            ),
            PolarizerOpts::Angle { angle } => (
                main_messaging::polarizer::Command::PolarizerCustomAngle as i32,
                Some(angle),
            ),
            PolarizerOpts::Calibrate => (
                main_messaging::polarizer::Command::PolarizerCalibrateHome as i32,
                None,
            ),
            PolarizerOpts::Stress { .. } | PolarizerOpts::Settings { .. } => {
                unreachable!("Stress test and settings are handled separately")
            }
        };

        match self
            .send(McuPayload::ToMain(
                main_messaging::jetson_to_mcu::Payload::Polarizer(
                    main_messaging::Polarizer {
                        command,
                        angle_decidegrees: angle.unwrap_or(0),
                        speed: 0,
                        shortest_path: false,
                    },
                ),
            ))
            .await
        {
            Ok(CommonAckError::Success) => {
                info!("💈 Polarizer command {:?}: ack received", opts);
            }
            Ok(e) => {
                return Err(eyre!("Error for command {:?}: ack {:?}", opts, e));
            }
            Err(e) => {
                return Err(eyre!("Error for command {:?}: {:?}", opts, e));
            }
        }

        match tokio::time::timeout(
            Duration::from_secs(
                if command == main_messaging::polarizer::Command::PolarizerHome as i32
                    || command
                        == main_messaging::polarizer::Command::PolarizerCalibrateHome
                            as i32
                {
                    10
                } else {
                    2
                },
            ),
            self.wait_for_polarizer_wheel_state(),
        )
        .await
        {
            Ok(Ok(state)) => {
                info!("💈 Polarizer command {:?}: success", opts);
                debug!("Polarizer wheel state: {:?}", state);
            }
            Ok(Err(e)) => {
                return Err(eyre!(
                    "Error waiting for PolarizerWheelState for command {:?}: {:?}",
                    opts,
                    e
                ));
            }
            Err(_) => {
                return Err(eyre!(
                    "Timeout waiting for PolarizerWheelState message for command {:?}",
                    opts
                ));
            }
        }

        Ok(())
    }

    pub(crate) async fn polarizer_settings(
        &mut self,
        acceleration: u32,
        max_speed: u32,
    ) -> Result<()> {
        match self
            .send(McuPayload::ToMain(
                main_messaging::jetson_to_mcu::Payload::PolarizerWheelSettings(
                    main_messaging::PolarizerWheelSettings {
                        acceleration_steps_per_s2: acceleration,
                        max_speed_ms_per_turn: max_speed,
                    },
                ),
            ))
            .await
        {
            Ok(CommonAckError::Success) => {
                info!(
                    "💈 Polarizer settings: acceleration={}, max_speed={}",
                    acceleration, max_speed
                );
                Ok(())
            }
            Ok(e) => Err(eyre!("Error setting polarizer settings: ack {:?}", e)),
            Err(e) => Err(eyre!("Error setting polarizer settings: {:?}", e)),
        }
    }

    pub(crate) async fn polarizer_stress(
        &mut self,
        speed: u32,
        repeat: u32,
        random: bool,
    ) -> Result<()> {
        let positions = [
            (
                "passthrough",
                main_messaging::polarizer::Command::PolarizerPassThrough as i32,
            ),
            (
                "vertical",
                main_messaging::polarizer::Command::Polarizer90Vertical as i32,
            ),
            (
                "horizontal",
                main_messaging::polarizer::Command::Polarizer0Horizontal as i32,
            ),
        ];

        let mut success_count = 0u32;
        let mut error_count = 0u32;
        info!(
            "💈 Starting polarizer stress test: speed={}, repeat={}, random={}",
            speed, repeat, random
        );

        // Build sequence of positions: either cycling or random
        let sequence: Vec<_> = if random {
            use rand::prelude::*;
            let mut rng = rand::thread_rng();
            let mut last_idx: Option<usize> = None;
            (0..repeat)
                .map(|_| {
                    let idx = loop {
                        let candidate = rng.gen_range(0..positions.len());
                        if last_idx != Some(candidate) {
                            break candidate;
                        }
                    };
                    last_idx = Some(idx);
                    positions[idx]
                })
                .collect()
        } else {
            positions
                .iter()
                .copied()
                .cycle()
                .take(repeat as usize)
                .collect()
        };

        for (i, (name, command)) in sequence.into_iter().enumerate() {
            // delay 1 second between commands
            tokio::time::sleep(Duration::from_secs(1)).await;

            let send_result = self
                .send(McuPayload::ToMain(
                    main_messaging::jetson_to_mcu::Payload::Polarizer(
                        main_messaging::Polarizer {
                            command,
                            angle_decidegrees: 0,
                            speed: if speed == 0 && command == main_messaging::polarizer::Command::PolarizerPassThrough as i32 { 3000 } else { speed },
                            shortest_path: false,
                        },
                    ),
                ))
                .await;
            match send_result {
                Ok(CommonAckError::Success) => {
                    debug!(
                        "[{}/{}] Polarizer -> {}: ack received",
                        i + 1,
                        repeat,
                        name
                    );
                }
                Ok(e) => {
                    error_count += 1;
                    error!(
                        "[{}/{}] Polarizer -> {}: ack error {:?}",
                        i + 1,
                        repeat,
                        name,
                        e
                    );
                    continue;
                }
                Err(e) => {
                    error_count += 1;
                    error!(
                        "[{}/{}] Polarizer -> {}: error {:?}",
                        i + 1,
                        repeat,
                        name,
                        e
                    );
                    continue;
                }
            }

            // wait for wheel to be at commanded position
            // with 2-second timeout
            match tokio::time::timeout(
                Duration::from_secs(2),
                self.wait_for_polarizer_wheel_state(),
            )
            .await
            {
                Ok(Ok(state)) => {
                    success_count += 1;
                    debug!(
                        "[{}/{}] Polarizer -> {}: success [{:?}]",
                        i + 1,
                        repeat,
                        name,
                        state
                    );
                }
                Ok(Err(e)) => {
                    error_count += 1;
                    error!(
                        "[{}/{}] Polarizer -> {}: error waiting for state {:?}",
                        i + 1,
                        repeat,
                        name,
                        e
                    );
                }
                Err(_) => {
                    error_count += 1;
                    error!(
                        "[{}/{}] Polarizer -> {}: timeout waiting for state",
                        i + 1,
                        repeat,
                        name
                    );
                }
            }
        }

        info!(
            "💈 Polarizer stress test complete: ✅ {} success, ❌ {} errors",
            success_count, error_count
        );

        if error_count > 0 {
            return Err(eyre!(
                "Polarizer stress test had {} errors out of {} attempts",
                error_count,
                repeat
            ));
        }

        Ok(())
    }

    pub async fn front_leds(&mut self, leds: Leds) -> Result<()> {
        if let Leds::Booster = leds {
            match self
                .send(McuPayload::ToMain(
                    main_messaging::jetson_to_mcu::Payload::WhiteLedsBrightness(
                        main_messaging::WhiteLeDsBrightness {
                            brightness: 50, /* thousandth, so 0.5% */
                        },
                    ),
                ))
                .await
            {
                Ok(CommonAckError::Success) => {
                    info!("🚀 Booster LEDs enabled");
                }
                Ok(e) => {
                    return Err(eyre!("Error enabling booster LEDs: ack {:?}", e));
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
                Ok(CommonAckError::Success) => {
                    info!("🚦 {:?} enabled", pattern);
                }
                Ok(e) => {
                    return Err(eyre!("Error enabling green LEDs: ack {:?}", e));
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
                .send(McuPayload::ToMain(
                    main_messaging::jetson_to_mcu::Payload::WhiteLedsBrightness(
                        main_messaging::WhiteLeDsBrightness { brightness: 0 },
                    ),
                ))
                .await
            {
                Ok(CommonAckError::Success) => {
                    info!("LEDs disabled");
                }
                Ok(e) => {
                    return Err(eyre!("Error disabling booster LEDs: ack {:?}", e));
                }
                Err(e) => {
                    return Err(eyre!("Error disabling booster LEDs: {:?}", e));
                }
            }
        } else {
            match self
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
                Ok(CommonAckError::Success) => {
                    info!("LEDs disabled");
                }
                Ok(e) => {
                    return Err(eyre!("Error disabling RGB LEDs: ack {:?}", e));
                }
                Err(e) => {
                    return Err(eyre!("Error disabling RGB LEDs: {:?}", e));
                }
            }
        }

        Ok(())
    }

    pub async fn wifi_power_cycle(&mut self) -> Result<()> {
        match self
            .send(McuPayload::ToMain(
                main_messaging::jetson_to_mcu::Payload::PowerCycle(
                    main_messaging::PowerCycle {
                        line: main_messaging::power_cycle::Line::Wifi3v3 as i32,
                        duration_ms: 0, // use default
                    },
                ),
            ))
            .await
        {
            Ok(CommonAckError::Success) => { /* nothing */ }
            Ok(a) => {
                return Err(eyre!("error power cycling wifi: ack {a:?}"));
            }
            Err(e) => {
                return Err(eyre!("error power cycling wifi: {e:?}"));
            }
        }
        Ok(())
    }

    pub async fn heat_camera_power_cycle(&mut self) -> Result<()> {
        match self
            .send(McuPayload::ToMain(
                main_messaging::jetson_to_mcu::Payload::PowerCycle(
                    main_messaging::PowerCycle {
                        line: main_messaging::power_cycle::Line::HeatCamera2v8 as i32,
                        duration_ms: 0, // use default
                    },
                ),
            ))
            .await
        {
            Ok(CommonAckError::Success) => { /* nothing */ }
            Ok(a) => {
                return Err(eyre!("error power cycling heat camera (2v8): ack {a:?}"));
            }
            Err(e) => {
                return Err(eyre!("error power cycling heat camera (2v8): {e:?}"));
            }
        }
        Ok(())
    }

    async fn wait_for_polarizer_wheel_state(
        &mut self,
    ) -> Result<main_messaging::PolarizerWheelState> {
        loop {
            let Some(mcu_payload) = self.message_queue_rx.recv().await else {
                return Err(eyre!("message queue closed"));
            };
            let McuPayload::FromMain(main_mcu_payload) = mcu_payload else {
                continue;
            };

            if let main_messaging::mcu_to_jetson::Payload::PolarizerWheelState(state) =
                main_mcu_payload
            {
                return Ok(state);
            }
        }
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
        match self.send(reboot_msg).await {
            Ok(CommonAckError::Success) => {
                info!("🚦 Rebooting main microcontroller in {} seconds", delay);
                Ok(())
            }
            Ok(ack) => {
                Err(eyre!("Failed to reboot main microcontroller: ack: {ack:?}"))
            }
            Err(e) => Err(eyre!("Failed to reboot main microcontroller: {e:?}")),
        }
    }

    async fn fetch_info(&mut self, info: &mut OrbInfo, diag: bool) -> Result<()> {
        let mut board_info = MainBoardInfo::new()
            .build(self, Some(diag))
            .await
            .unwrap_or_else(|board_info| board_info);

        info.hw_rev = board_info.hw_version;
        info.main_fw_versions = board_info.fw_versions;
        info.main_battery_status = board_info.battery_status;
        info.hardware_states.append(&mut board_info.hardware_states);

        Ok(())
    }

    async fn dump(
        &mut self,
        duration: Option<Duration>,
        logs_only: bool,
    ) -> Result<()> {
        let until_time = duration.map(|d| std::time::Instant::now() + d);

        loop {
            if let Some(until_time) = until_time
                && std::time::Instant::now() > until_time
            {
                break;
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

    async fn update_firmware(&mut self, path: &str, force: bool) -> Result<()> {
        let buffer = dfu::load_binary_file(path)?;
        debug!("Sending file {} ({} bytes)", path, buffer.len());

        // Check if update should be skipped (unless forced)
        if !force {
            // Parse version from binary file
            let binary_version = dfu::parse_firmware_version(&buffer)?;
            info!("📦 Binary firmware version: {}", binary_version);

            let board_info = MainBoardInfo::new()
                .build(self, None)
                .await
                .unwrap_or_else(|board_info| board_info);

            if let Some(fw_versions) = &board_info.fw_versions {
                if let Some(primary_app) = &fw_versions.primary_app {
                    if binary_version.matches(primary_app) {
                        info!(
                            "⏭️  Skipping update: binary version {} matches current MCU version v{}.{}.{}-0x{:x}",
                            binary_version,
                            primary_app.major,
                            primary_app.minor,
                            primary_app.patch,
                            primary_app.commit_hash
                        );
                        info!("💡 Use --force to update anyway");

                        info!("🔁 Asking mcu to reboot gracefully");
                        let reboot_orb_msg = McuPayload::ToMain(
                            main_messaging::jetson_to_mcu::Payload::RebootOrb(
                                main_messaging::RebootOrb {
                                    force_reboot_timeout_s: 0, /* wait for jetson's graceful shutdown */
                                },
                            ),
                        );
                        match self.send(reboot_orb_msg).await {
                            Ok(CommonAckError::Success) => {
                                info!("🚦 The Orb will reboot once you shutdown the Jetson gracefully: `sudo shutdown now`");
                            }
                            Ok(e) => {
                                return Err(eyre!(
                                    "Error rebooting the orb: ack error: {:?}",
                                    e
                                ));
                            }
                            Err(e) => {
                                return Err(eyre!("Error rebooting the orb: {:?}", e));
                            }
                        }

                        return Ok(());
                    }
                    info!(
                        "🔄 Current MCU version: v{}.{}.{}-0x{:x}",
                        primary_app.major,
                        primary_app.minor,
                        primary_app.patch,
                        primary_app.commit_hash
                    );
                } else {
                    warn!("⚠️  Could not fetch primary app version from firmware versions, proceeding with update");
                }
            } else {
                warn!("⚠️  Could not fetch firmware versions, proceeding with update");
            }
        } else {
            warn!("⚠️  Force flag set, bypassing version check and performing update regardless of current firmware version");
        }

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

        self.switch_images(false).await?;

        info!("👉 Shut the Orb down to install the new image (`sudo shutdown now`), the Orb is going to reboot itself once installation is complete");

        Ok(())
    }

    async fn switch_images(&mut self, force: bool) -> Result<()> {
        if !force {
            let board_info = MainBoardInfo::new()
                .build(self, None)
                .await
                .unwrap_or_else(|board_info| board_info);

            if let Some(fw_versions) = board_info.fw_versions {
                if let Some(secondary_app) = fw_versions.secondary_app {
                    if let Some(primary_app) = fw_versions.primary_app {
                        if (primary_app.commit_hash == 0
                            && secondary_app.commit_hash != 0)
                            || (primary_app.commit_hash != 0
                                && secondary_app.commit_hash == 0)
                        {
                            return Err(eyre!("Primary and secondary images types (prod or dev) don't match"));
                        } else {
                            debug!("Primary and secondary images types (prod or dev) match");
                        }
                    } else {
                        return Err(eyre!(
                            "Firmware versions can't be verified: unknown primary app"
                        ));
                    }
                } else {
                    return Err(eyre!(
                        "Firmware versions can't be verified: unknown secondary app"
                    ));
                }
            } else {
                return Err(eyre!("Firmware versions can't be verified: board_info.fw_versions is None"));
            }
        } else {
            warn!("⚠️ Forcing image switch without preliminary checks");
        };

        let payload = McuPayload::ToMain(
            main_messaging::jetson_to_mcu::Payload::FwImageSecondaryActivate(
                orb_messages::FirmwareActivateSecondary {
                    force_permanent: false,
                },
            ),
        );
        match self.send(payload).await {
            Ok(CommonAckError::Success) => {
                info!("✅ Image activated for installation after reboot (use `sudo shutdown now` to gracefully install the image)");
            }
            Ok(ack_error) => {
                return Err(eyre!("Unable to activate image: ack: {:?}", ack_error));
            }
            Err(e) => {
                return Err(eyre!("Unable to activate image: {:?}", e));
            }
        }

        Ok(())
    }

    async fn stress_test(&mut self, duration: Option<Duration>) -> Result<()> {
        let has_isotp = self.isotp_iface.is_some();
        let test_count: u32 = if has_isotp { 2 } else { 1 };
        let mut test_idx: u32 = 0;
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
                    0 if has_isotp => {
                        self.isotp_iface.as_mut().unwrap().send(payload).await
                    }
                    _ => self.canfd_iface.send(payload).await,
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
            let test_name = if test_idx == 0 && has_isotp {
                "ISO-TP"
            } else {
                "CAN-FD"
            };
            info!(
                "📈 {} #{:8}\t⚡️ {:4} v/s\t\t✅ {:}%\t\t❌ {:}%\t[{}]",
                test_name,
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
    hardware_states: Vec<orb_messages::HardwareState>,
}

impl MainBoardInfo {
    fn new() -> Self {
        Self {
            hw_version: None,
            fw_versions: None,
            battery_status: None,
            hardware_states: vec![],
        }
    }

    /// Fetches `MainBoardInfo` from the main board
    /// doesn't fail, but lazily fetches as much info as it could
    /// on timeout, returns the info that was fetched so far
    async fn build(
        mut self,
        main_board: &mut MainBoard,
        diag: Option<bool>,
    ) -> Result<Self, Self> {
        let mut is_err = false;

        // Send one message over can-fd to have the jetson receive all the
        // broadcast messages like battery stats.
        // For that, a subscriber needs to be added by sending one message;
        // in case no other process sent a message over can-fd
        let _ = main_board
            .canfd_iface
            .send(McuPayload::ToMain(
                main_messaging::jetson_to_mcu::Payload::ValueGet(
                    orb_messages::ValueGet {
                        value: orb_messages::value_get::Value::FirmwareVersions as i32,
                    },
                ),
            ))
            .await;

        match main_board
            .send(McuPayload::ToMain(
                main_messaging::jetson_to_mcu::Payload::ValueGet(
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

        match main_board
            .send(McuPayload::ToMain(
                main_messaging::jetson_to_mcu::Payload::ValueGet(
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

        if let Some(true) = diag {
            match main_board
                .send(McuPayload::ToMain(
                    main_messaging::jetson_to_mcu::Payload::SyncDiagData(
                        orb_messages::SyncDiagData {
                            interval: 0, // use default
                        },
                    ),
                ))
                .await
            {
                Ok(CommonAckError::Success) => { /* nothing */ }
                Ok(a) => {
                    error!("error asking for diag data: {a:?}");
                }
                Err(e) => {
                    error!("error asking for diag data: {e:?}");
                }
            }
        }

        /* listen_for_board_info will return when all info is populated, or if `diag`
         * is enabled, will wait until timeout to receive all the diag data.
         */
        match tokio::time::timeout(
            Duration::from_secs(2),
            self.listen_for_board_info(main_board, diag.unwrap_or(false)),
        )
        .await
        {
            Err(tokio::time::error::Elapsed { .. }) => {
                if !diag.unwrap_or(false) {
                    warn!("Timeout waiting on main board info");
                    is_err = true;
                } else {
                    debug!("Main board info should be entirely received by now, with diag data");
                }
            }
            Ok(()) => {
                debug!("Got main board info");
            }
        }

        if is_err {
            Err(self)
        } else {
            Ok(self)
        }
    }

    /// Mutates `self` while listening for board info messages.
    ///
    /// Does not terminate until all board info is populated.
    async fn listen_for_board_info(&mut self, main_board: &mut MainBoard, diag: bool) {
        let mut battery_status = BatteryStatus {
            percentage: None,
            voltage_mv: None,
            is_charging: None,
            is_corded: None,
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
                    if b.corded_power_supply_mv != 0 {
                        battery_status.voltage_mv =
                            Some(b.corded_power_supply_mv as u32);
                        battery_status.is_corded = Some(true);
                    } else {
                        battery_status.voltage_mv = Some(
                            (b.battery_cell1_mv
                                + b.battery_cell2_mv
                                + b.battery_cell3_mv
                                + b.battery_cell4_mv)
                                as u32,
                        );
                        battery_status.is_corded = Some(false);
                    }
                }
                main_messaging::mcu_to_jetson::Payload::BatteryIsCharging(b) => {
                    battery_status.is_charging = Some(b.battery_is_charging);
                }
                main_messaging::mcu_to_jetson::Payload::HwState(h) => {
                    self.hardware_states.push(h);
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
            if !diag
                && self.hw_version.is_some()
                && self.fw_versions.is_some()
                && self.battery_status.is_some()
            {
                return;
            }
        }
    }
}
