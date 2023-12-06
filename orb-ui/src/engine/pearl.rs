use async_trait::async_trait;
use std::f64::consts::PI;
use std::time::Duration;

use eyre::Result;
use futures::channel::mpsc;
use orb_mcu_messaging::mcu_message::Message;
use orb_mcu_messaging::{jetson_to_mcu, JetsonToMcu};
use tokio::time;

use pid::{InstantTimer, Timer};

use crate::engine::rgb::Rgb;
use crate::engine::{
    center, operator, ring, Animation, AnimationsStack, CenterFrame, Event,
    EventHandler, OperatorFrame, QrScanSchema, RingFrame, Runner, RunningAnimation,
    BIOMETRIC_PIPELINE_MAX_PROGRESS, LEVEL_BACKGROUND, LEVEL_FOREGROUND, LEVEL_NOTICE,
    PEARL_CENTER_LED_COUNT, PEARL_RING_LED_COUNT,
};

struct WrappedMessage(Message);

impl From<CenterFrame<PEARL_CENTER_LED_COUNT>> for WrappedMessage {
    fn from(value: CenterFrame<PEARL_CENTER_LED_COUNT>) -> Self {
        WrappedMessage(Message::JMessage(
            JetsonToMcu {
                ack_number: 0,
                payload: Some(jetson_to_mcu::Payload::CenterLedsSequence(
                    orb_mcu_messaging::UserCenterLeDsSequence {
                        data_format: Some(
                            orb_mcu_messaging::user_center_le_ds_sequence::DataFormat::RgbUncompressed(
                                value.iter().flat_map(|&Rgb(r, g, b)| [r, g, b]).collect(),
                            ))
                    }
                )),
            }
        ))
    }
}

impl From<RingFrame<PEARL_RING_LED_COUNT>> for WrappedMessage {
    fn from(value: RingFrame<PEARL_RING_LED_COUNT>) -> Self {
        WrappedMessage(Message::JMessage(
            JetsonToMcu {
                ack_number: 0,
                payload: Some(jetson_to_mcu::Payload::RingLedsSequence(
                    orb_mcu_messaging::UserRingLeDsSequence {
                        data_format: Some(
                            orb_mcu_messaging::user_ring_le_ds_sequence::DataFormat::RgbUncompressed(
                                value.iter().flat_map(|&Rgb(r, g, b)| [r, g, b]).collect(),
                            ))
                    }
                )),
            }
        ))
    }
}

impl From<OperatorFrame> for WrappedMessage {
    fn from(value: OperatorFrame) -> Self {
        WrappedMessage(Message::JMessage(
            JetsonToMcu {
                ack_number: 0,
                payload: Some(jetson_to_mcu::Payload::DistributorLedsSequence(
                    orb_mcu_messaging::DistributorLeDsSequence {
                        data_format: Some(
                            orb_mcu_messaging::distributor_le_ds_sequence::DataFormat::RgbUncompressed(
                                value.iter().flat_map(|&Rgb(r, g, b)| [r, g, b]).collect(),
                            ))
                    }
                )),
            }
        ))
    }
}

impl Runner<PEARL_RING_LED_COUNT, PEARL_CENTER_LED_COUNT> {
    pub(crate) fn new() -> Self {
        Self {
            timer: InstantTimer::default(),
            ring_animations_stack: AnimationsStack::new(),
            center_animations_stack: AnimationsStack::new(),
            ring_frame: [Rgb(0, 0, 0); PEARL_RING_LED_COUNT],
            center_frame: [Rgb(0, 0, 0); PEARL_CENTER_LED_COUNT],
            operator_frame: OperatorFrame::default(),
            operator_connection: operator::Connection::default(),
            operator_battery: operator::Battery::default(),
            operator_blink: operator::Blink::default(),
            operator_pulse: operator::Pulse::default(),
            operator_action: operator::Bar::default(),
            operator_signup_phase: operator::SignupPhase::default(),
            paused: false,
        }
    }

    fn set_ring(
        &mut self,
        level: u8,
        animation: impl Animation<Frame = RingFrame<PEARL_RING_LED_COUNT>>,
    ) {
        self.ring_animations_stack.set(level, Box::new(animation));
    }

    fn set_center(
        &mut self,
        level: u8,
        animation: impl Animation<Frame = CenterFrame<PEARL_CENTER_LED_COUNT>>,
    ) {
        self.center_animations_stack.set(level, Box::new(animation));
    }

    fn stop_ring(&mut self, level: u8, force: bool) {
        self.ring_animations_stack.stop(level, force);
    }

    fn stop_center(&mut self, level: u8, force: bool) {
        self.center_animations_stack.stop(level, force);
    }
}

#[async_trait]
impl EventHandler for Runner<PEARL_RING_LED_COUNT, PEARL_CENTER_LED_COUNT> {
    #[allow(clippy::too_many_lines)]
    fn event(&mut self, event: &Event) {
        tracing::debug!("LED event: {:?}", event);

        match event {
            Event::Bootup => {
                self.stop_ring(LEVEL_NOTICE, true);
                self.stop_center(LEVEL_NOTICE, true);
                self.set_ring(
                    LEVEL_BACKGROUND,
                    ring::Idle::<PEARL_RING_LED_COUNT>::default(),
                );
                self.operator_pulse.trigger(2048.0, 1., 1., false);
            }
            Event::BootComplete => self.operator_pulse.stop(),
            Event::Shutdown { requested } => {
                self.set_center(
                    LEVEL_NOTICE,
                    center::Alert::<PEARL_CENTER_LED_COUNT>::new(
                        if *requested {
                            Rgb::USER_QR_SCAN
                        } else {
                            Rgb::USER_AMBER
                        },
                        vec![0.0, 0.3, 0.45, 0.3, 0.45, 0.45],
                        false,
                    ),
                );
                self.operator_action
                    .trigger(1.0, Rgb::OFF, true, false, true);
            }
            Event::SignupStart => {
                // starting signup sequence, operator LEDs in blue
                // animate from left to right (`operator_action`)
                // and then keep first LED on as a background (`operator_signup_phase`)
                self.operator_action.trigger(
                    0.6,
                    Rgb::OPERATOR_DEFAULT,
                    false,
                    true,
                    false,
                );
                self.operator_signup_phase.signup_phase_started();

                // clear user animations
                self.stop_ring(LEVEL_FOREGROUND, true);
                self.stop_center(LEVEL_FOREGROUND, true);
                self.stop_ring(LEVEL_NOTICE, true);
                self.stop_center(LEVEL_NOTICE, true);
            }
            Event::QrScanStart { schema } => {
                self.set_center(
                    LEVEL_FOREGROUND,
                    center::Wave::<PEARL_CENTER_LED_COUNT>::new(
                        Rgb::USER_QR_SCAN,
                        5.0,
                        0.5,
                        true,
                    ),
                );

                match schema {
                    QrScanSchema::Operator => {
                        self.operator_signup_phase.operator_qr_code_ok();
                    }
                    QrScanSchema::Wifi => {
                        self.operator_connection.no_wlan();
                    }
                    QrScanSchema::User => {
                        self.operator_signup_phase.user_qr_code_ok();
                        // initialize ring with short segment to invite user to scan QR
                        self.set_ring(
                            LEVEL_FOREGROUND,
                            ring::Slider::<PEARL_RING_LED_COUNT>::new(
                                0.0,
                                Rgb::USER_SIGNUP,
                            ),
                        );
                    }
                };
            }
            Event::QrScanCompleted { schema: _ } => {
                self.set_center(
                    LEVEL_NOTICE,
                    center::Alert::<PEARL_CENTER_LED_COUNT>::new(
                        Rgb::USER_QR_SCAN,
                        vec![0.0, 0.3, 0.45, 0.46],
                        false,
                    ),
                );
                self.stop_center(LEVEL_FOREGROUND, true);
            }
            Event::QrScanUnexpected { schema } => {
                match schema {
                    QrScanSchema::User => {
                        self.operator_signup_phase.user_qr_code_issue();
                    }
                    QrScanSchema::Operator => {
                        self.operator_signup_phase.operator_qr_code_issue();
                    }
                    QrScanSchema::Wifi => {}
                }
                self.stop_center(LEVEL_FOREGROUND, true);
            }
            Event::QrScanFail { schema } => {
                match schema {
                    QrScanSchema::User | QrScanSchema::Operator => {
                        self.stop_center(LEVEL_FOREGROUND, true);
                        self.set_center(
                            LEVEL_FOREGROUND,
                            center::Static::<PEARL_CENTER_LED_COUNT>::new(
                                Rgb::OFF,
                                None,
                            ),
                        );
                        self.operator_signup_phase.failure();
                    }
                    QrScanSchema::Wifi => {}
                }
                self.stop_ring(LEVEL_FOREGROUND, true);
            }
            Event::QrScanSuccess { schema } => {
                if matches!(schema, QrScanSchema::Operator) {
                    self.operator_signup_phase.operator_qr_captured();
                } else if matches!(schema, QrScanSchema::User) {
                    self.operator_signup_phase.user_qr_captured();
                    // initialize ring with short segment to invite user to start iris capture
                    self.set_ring(
                        LEVEL_NOTICE,
                        ring::Slider::<PEARL_RING_LED_COUNT>::new(
                            0.0,
                            Rgb::USER_SIGNUP,
                        )
                        .pulse_remaining(),
                    );
                    // off background for biometric-capture, which relies on LEVEL_NOTICE animations
                    self.stop_center(LEVEL_FOREGROUND, true);
                    self.set_center(
                        LEVEL_FOREGROUND,
                        center::Static::<PEARL_CENTER_LED_COUNT>::new(Rgb::OFF, None),
                    );
                }
                self.stop_ring(LEVEL_FOREGROUND, true);
            }
            Event::BiometricCaptureHalfObjectivesCompleted => {
                // do nothing
            }
            Event::BiometricCaptureAllObjectivesCompleted => {
                self.operator_signup_phase.irises_captured();
            }
            Event::BiometricCaptureProgress { progress } => {
                if self
                    .ring_animations_stack
                    .stack
                    .get_mut(&LEVEL_NOTICE)
                    .and_then(|RunningAnimation { animation, .. }| {
                        animation
                            .as_any_mut()
                            .downcast_mut::<ring::Slider<PEARL_RING_LED_COUNT>>()
                    })
                    .is_none()
                {
                    // in case animation not yet initialized through user QR scan success event
                    // initialize ring with short segment to invite user to start iris capture
                    self.set_ring(
                        LEVEL_NOTICE,
                        ring::Slider::<PEARL_RING_LED_COUNT>::new(
                            0.0,
                            Rgb::USER_SIGNUP,
                        )
                        .pulse_remaining(),
                    );
                }
                let ring_progress = self
                    .ring_animations_stack
                    .stack
                    .get_mut(&LEVEL_NOTICE)
                    .and_then(|RunningAnimation { animation, .. }| {
                        animation
                            .as_any_mut()
                            .downcast_mut::<ring::Slider<PEARL_RING_LED_COUNT>>()
                    });
                if let Some(ring_progress) = ring_progress {
                    ring_progress.set_progress(*progress, true);
                }
            }
            Event::BiometricCaptureOcclusion { occlusion_detected } => {
                if *occlusion_detected {
                    self.operator_signup_phase.capture_occlusion_issue();
                } else {
                    self.operator_signup_phase.capture_occlusion_ok();
                }
            }
            Event::BiometricCaptureDistance { in_range } => {
                if *in_range {
                    self.operator_signup_phase.capture_distance_ok();
                } else {
                    self.operator_signup_phase.capture_distance_issue();
                }
            }
            Event::BiometricCaptureSuccess => {
                // set ring to full circle based on previous progress animation
                // ring will be reset when biometric pipeline starts showing progress
                let _ = self
                    .ring_animations_stack
                    .stack
                    .get_mut(&LEVEL_NOTICE)
                    .and_then(|RunningAnimation { animation, .. }| {
                        animation
                            .as_any_mut()
                            .downcast_mut::<ring::Slider<PEARL_RING_LED_COUNT>>()
                    })
                    .map(|x| {
                        x.set_progress(1.0, false);
                    });
                self.stop_center(LEVEL_NOTICE, true);
                self.operator_signup_phase.iris_scan_complete();
            }
            Event::BiometricPipelineProgress { progress } => {
                let ring_animation = self
                    .ring_animations_stack
                    .stack
                    .get_mut(&LEVEL_FOREGROUND)
                    .and_then(|RunningAnimation { animation, .. }| {
                        animation
                            .as_any_mut()
                            .downcast_mut::<ring::Progress<PEARL_RING_LED_COUNT>>()
                    });
                if let Some(ring_animation) = ring_animation {
                    ring_animation.set_progress(
                        *progress * BIOMETRIC_PIPELINE_MAX_PROGRESS,
                        None,
                    );
                } else {
                    self.set_ring(
                        LEVEL_FOREGROUND,
                        ring::Progress::<PEARL_RING_LED_COUNT>::new(
                            0.0,
                            None,
                            Rgb::USER_SIGNUP,
                        ),
                    );
                }

                // operator LED to show pipeline progress
                if *progress <= 0.5 {
                    self.operator_signup_phase.processing_1();
                } else {
                    self.operator_signup_phase.processing_2();
                }
            }
            Event::StartingEnrollment => {
                let slider = self
                    .ring_animations_stack
                    .stack
                    .get_mut(&LEVEL_FOREGROUND)
                    .and_then(|RunningAnimation { animation, .. }| {
                        animation
                            .as_any_mut()
                            .downcast_mut::<ring::Progress<PEARL_RING_LED_COUNT>>()
                    });
                if let Some(slider) = slider {
                    slider.set_pulse_angle(PI / 180.0 * 20.0);
                }
                self.operator_signup_phase.uploading();
            }
            Event::BiometricPipelineSuccess => {
                let slider = self
                    .ring_animations_stack
                    .stack
                    .get_mut(&LEVEL_FOREGROUND)
                    .and_then(|RunningAnimation { animation, .. }| {
                        animation
                            .as_any_mut()
                            .downcast_mut::<ring::Progress<PEARL_RING_LED_COUNT>>()
                    });
                if let Some(slider) = slider {
                    slider.set_progress(BIOMETRIC_PIPELINE_MAX_PROGRESS, None);
                }
                self.operator_signup_phase.biometric_pipeline_successful();
            }
            Event::SignupFail => {
                self.operator_signup_phase.failure();

                let slider = self
                    .ring_animations_stack
                    .stack
                    .get_mut(&LEVEL_FOREGROUND)
                    .and_then(|RunningAnimation { animation, .. }| {
                        animation
                            .as_any_mut()
                            .downcast_mut::<ring::Progress<PEARL_RING_LED_COUNT>>()
                    });
                if let Some(slider) = slider {
                    slider.set_progress(2.0, None);
                }
                self.stop_ring(LEVEL_FOREGROUND, false);
                self.stop_ring(LEVEL_NOTICE, true);
                self.stop_center(LEVEL_NOTICE, true);
            }
            Event::SignupUnique => {
                self.operator_signup_phase.signup_successful();

                let slider = self
                    .ring_animations_stack
                    .stack
                    .get_mut(&LEVEL_FOREGROUND)
                    .and_then(|RunningAnimation { animation, .. }| {
                        animation
                            .as_any_mut()
                            .downcast_mut::<ring::Progress<PEARL_RING_LED_COUNT>>()
                    });
                if let Some(slider) = slider {
                    slider.set_progress(2.0, None);
                }
                self.stop_ring(LEVEL_FOREGROUND, false);

                self.stop_ring(LEVEL_NOTICE, true);
                self.stop_center(LEVEL_NOTICE, true);
                self.set_ring(
                    LEVEL_FOREGROUND,
                    ring::Idle::<PEARL_RING_LED_COUNT>::new(
                        Some(Rgb::USER_SIGNUP),
                        Some(3.0),
                    ),
                );
            }
            Event::SoftwareVersionDeprecated => {
                let slider = self
                    .ring_animations_stack
                    .stack
                    .get_mut(&LEVEL_FOREGROUND)
                    .and_then(|RunningAnimation { animation, .. }| {
                        animation
                            .as_any_mut()
                            .downcast_mut::<ring::Progress<PEARL_RING_LED_COUNT>>()
                    });
                if let Some(slider) = slider {
                    slider.set_progress(2.0, None);
                }
                self.stop_ring(LEVEL_FOREGROUND, false);
                self.operator_blink.trigger(
                    Rgb::OPERATOR_VERSIONS_DEPRECATED,
                    vec![0.4, 0.4, 0.4, 0.4, 0.4, 0.4],
                );
            }
            Event::SoftwareVersionBlocked => {
                let slider = self
                    .ring_animations_stack
                    .stack
                    .get_mut(&LEVEL_FOREGROUND)
                    .and_then(|RunningAnimation { animation, .. }| {
                        animation
                            .as_any_mut()
                            .downcast_mut::<ring::Progress<PEARL_RING_LED_COUNT>>()
                    });
                if let Some(slider) = slider {
                    slider.set_progress(2.0, None);
                }
                self.stop_ring(LEVEL_FOREGROUND, false);
                self.operator_blink.trigger(
                    Rgb::OPERATOR_VERSIONS_OUTDATED,
                    vec![0.4, 0.4, 0.4, 0.4, 0.4, 0.4],
                );
            }
            Event::Idle => {
                self.stop_ring(LEVEL_FOREGROUND, false);
                self.stop_center(LEVEL_FOREGROUND, false);
                self.operator_signup_phase.idle();
            }
            Event::GoodInternet => {
                self.operator_connection.good_internet();
            }
            Event::SlowInternet => {
                self.operator_connection.slow_internet();
            }
            Event::NoInternet => {
                self.operator_connection.no_internet();
            }
            Event::GoodWlan => {
                self.operator_connection.good_wlan();
            }
            Event::SlowWlan => {
                self.operator_connection.slow_wlan();
            }
            Event::NoWlan => {
                self.operator_connection.no_wlan();
            }
            Event::BatteryCapacity { percentage } => {
                self.operator_battery.capacity(*percentage);
            }
            Event::BatteryIsCharging { is_charging } => {
                self.operator_battery.set_charging(*is_charging);
            }
            Event::Pause => {
                self.paused = true;
            }
            Event::Resume => {
                self.paused = false;
            }
            Event::RecoveryImage => {
                self.set_ring(
                    LEVEL_NOTICE,
                    ring::Spinner::<PEARL_RING_LED_COUNT>::triple(Rgb::USER_RED),
                );
            }
        }
    }

    async fn run(&mut self, interface_tx: &mut mpsc::Sender<Message>) -> Result<()> {
        let dt = self.timer.get_dt().unwrap_or(0.0);
        self.center_animations_stack.run(&mut self.center_frame, dt);
        if !self.paused {
            interface_tx.try_send(WrappedMessage::from(self.center_frame).0)?;
        }

        self.operator_battery
            .animate(&mut self.operator_frame, dt, false);
        self.operator_connection
            .animate(&mut self.operator_frame, dt, false);
        self.operator_signup_phase
            .animate(&mut self.operator_frame, dt, false);
        self.operator_blink
            .animate(&mut self.operator_frame, dt, false);
        self.operator_pulse
            .animate(&mut self.operator_frame, dt, false);
        self.operator_action
            .animate(&mut self.operator_frame, dt, false);
        time::sleep(Duration::from_millis(2)).await;
        if !self.paused {
            interface_tx.try_send(WrappedMessage::from(self.operator_frame).0)?;
        }

        self.ring_animations_stack.run(&mut self.ring_frame, dt);
        time::sleep(Duration::from_millis(2)).await;
        if !self.paused {
            interface_tx.try_send(WrappedMessage::from(self.ring_frame).0)?;
        }
        Ok(())
    }
}
