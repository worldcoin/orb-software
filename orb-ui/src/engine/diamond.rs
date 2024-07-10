use async_trait::async_trait;
use eyre::Result;
use futures::channel::mpsc::Sender;
use futures::future::Either;
use futures::{future, StreamExt};
use orb_messages::mcu_main::mcu_message::Message;
use orb_messages::mcu_main::{jetson_to_mcu, JetsonToMcu};
use pid::{InstantTimer, Timer};
use std::f64::consts::PI;
use std::time::Duration;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::time;
use tokio_stream::wrappers::{IntervalStream, UnboundedReceiverStream};

use crate::engine::rgb::Argb;
use crate::engine::{
    center, operator, ring, Animation, AnimationsStack, CenterFrame, ConeFrame, Event,
    EventHandler, OperatorFrame, OrbType, QrScanSchema, QrScanUnexpectedReason,
    RingFrame, Runner, RunningAnimation, SignupFailReason, DIAMOND_CENTER_LED_COUNT,
    DIAMOND_CONE_LED_COUNT, DIAMOND_RING_LED_COUNT, LED_ENGINE_FPS, LEVEL_BACKGROUND,
    LEVEL_FOREGROUND, LEVEL_NOTICE,
};
use crate::sound;
use crate::sound::Player;

struct WrappedCenterMessage(Message);
struct WrappedRingMessage(Message);
struct WrappedConeMessage(Message);
struct WrappedOperatorMessage(Message);

impl From<CenterFrame<DIAMOND_CENTER_LED_COUNT>> for WrappedCenterMessage {
    fn from(value: CenterFrame<DIAMOND_CENTER_LED_COUNT>) -> Self {
        WrappedCenterMessage(Message::JMessage(
            JetsonToMcu {
                ack_number: 0,
                payload: Some(jetson_to_mcu::Payload::CenterLedsSequence(
                    orb_messages::mcu_main::UserCenterLeDsSequence {
                        data_format: Some(
                            orb_messages::mcu_main::user_center_le_ds_sequence::DataFormat::Argb32Uncompressed(
                                value.iter().rev().flat_map(|&Argb(a, r, g, b)| [a.unwrap_or(0_u8), r, g, b]).collect(),
                            ))
                    }
                )),
            }
        ))
    }
}

impl From<RingFrame<DIAMOND_RING_LED_COUNT>> for WrappedRingMessage {
    fn from(value: RingFrame<DIAMOND_RING_LED_COUNT>) -> Self {
        WrappedRingMessage(Message::JMessage(
            JetsonToMcu {
                ack_number: 0,
                payload: Some(jetson_to_mcu::Payload::RingLedsSequence(
                    orb_messages::mcu_main::UserRingLeDsSequence {
                        data_format: Some(
                            orb_messages::mcu_main::user_ring_le_ds_sequence::DataFormat::Argb32Uncompressed(
                                value.iter().rev().flat_map(|&Argb(a, r, g, b)| [a.unwrap_or(0_u8), r, g, b]).collect(),
                            ))
                    }
                )),
            }
        ))
    }
}

impl From<ConeFrame<DIAMOND_CONE_LED_COUNT>> for WrappedConeMessage {
    fn from(value: ConeFrame<DIAMOND_CONE_LED_COUNT>) -> Self {
        WrappedConeMessage(Message::JMessage(
            JetsonToMcu {
                ack_number: 0,
                payload: Some(jetson_to_mcu::Payload::ConeLedsSequence(
                    orb_messages::mcu_main::ConeLeDsSequence {
                        data_format: Some(
                            orb_messages::mcu_main::cone_le_ds_sequence::DataFormat::Argb32Uncompressed(
                                value.iter().flat_map(|&Argb(a, r, g, b)| [a.unwrap_or(0_u8), r, g, b]).collect(),
                            ))
                    }
                )),
            }
        ))
    }
}

impl From<OperatorFrame> for WrappedOperatorMessage {
    fn from(value: OperatorFrame) -> Self {
        WrappedOperatorMessage(Message::JMessage(
            JetsonToMcu {
                ack_number: 0,
                payload: Some(jetson_to_mcu::Payload::DistributorLedsSequence(
                    orb_messages::mcu_main::DistributorLeDsSequence {
                        data_format: Some(
                            orb_messages::mcu_main::distributor_le_ds_sequence::DataFormat::Argb32Uncompressed(
                                value.iter().rev().flat_map(|&Argb(a, r, g, b)| [a.unwrap_or(0_u8), r, g, b]).collect(),
                            ))
                    }
                )),
            }
        ))
    }
}

pub async fn event_loop(
    rx: UnboundedReceiver<Event>,
    mcu_tx: Sender<Message>,
) -> Result<()> {
    let mut interval = time::interval(Duration::from_millis(1000 / LED_ENGINE_FPS));
    interval.set_missed_tick_behavior(time::MissedTickBehavior::Delay);
    let mut interval = IntervalStream::new(interval);
    let mut rx = UnboundedReceiverStream::new(rx);
    let mut runner = match sound::Jetson::spawn().await {
        Ok(sound) => {
            Runner::<DIAMOND_RING_LED_COUNT, DIAMOND_CENTER_LED_COUNT>::new(sound)
        }
        Err(e) => {
            return {
                tracing::error!("Failed to initialize sound: {:?}", e);
                Err(e)
            };
        }
    };
    loop {
        match future::select(rx.next(), interval.next()).await {
            Either::Left((None, _)) => {
                break;
            }
            Either::Left((Some(event), _)) => {
                if let Err(e) = runner.event(&event) {
                    tracing::error!("Error handling event: {:?}", e);
                }
            }
            Either::Right(_) => {
                if let Err(e) = runner.run(&mut mcu_tx.clone()).await {
                    tracing::error!("Error running UI: {:?}", e);
                }
            }
        }
    }
    Ok(())
}

impl Runner<DIAMOND_RING_LED_COUNT, DIAMOND_CENTER_LED_COUNT> {
    pub(crate) fn new(sound: sound::Jetson) -> Self {
        Self {
            timer: InstantTimer::default(),
            ring_animations_stack: AnimationsStack::new(),
            center_animations_stack: AnimationsStack::new(),
            cone_animations_stack: Some(AnimationsStack::new()),
            ring_frame: [Argb(Some(0), 0, 0, 0); DIAMOND_RING_LED_COUNT],
            cone_frame: None,
            center_frame: [Argb(Some(0), 0, 0, 0); DIAMOND_CENTER_LED_COUNT],
            operator_frame: OperatorFrame::default(),
            operator_idle: operator::Idle::new(OrbType::Diamond),
            operator_blink: operator::Blink::new(OrbType::Diamond),
            operator_pulse: operator::Pulse::new(OrbType::Diamond),
            operator_action: operator::Bar::new(OrbType::Diamond),
            operator_signup_phase: operator::SignupPhase::new(OrbType::Diamond),
            sound,
            capture_sound: sound::capture::CaptureLoopSound::default(),
            paused: false,
        }
    }

    fn set_ring(
        &mut self,
        level: u8,
        animation: impl Animation<Frame = RingFrame<DIAMOND_RING_LED_COUNT>>,
    ) {
        self.ring_animations_stack.set(level, Box::new(animation));
    }

    fn set_cone(
        &mut self,
        level: u8,
        animation: impl Animation<Frame = ConeFrame<DIAMOND_CONE_LED_COUNT>>,
    ) {
        if let Some(animations) = &mut self.cone_animations_stack {
            animations.set(level, Box::new(animation));
        }
    }

    fn set_center(
        &mut self,
        level: u8,
        animation: impl Animation<Frame = CenterFrame<DIAMOND_CENTER_LED_COUNT>>,
    ) {
        self.center_animations_stack.set(level, Box::new(animation));
    }

    fn stop_ring(&mut self, level: u8, force: bool) {
        self.ring_animations_stack.stop(level, force);
    }

    fn stop_cone(&mut self, level: u8, force: bool) {
        if let Some(animations) = &mut self.cone_animations_stack {
            animations.stop(level, force);
        }
    }

    fn stop_center(&mut self, level: u8, force: bool) {
        self.center_animations_stack.stop(level, force);
    }
}

#[async_trait]
impl EventHandler for Runner<DIAMOND_RING_LED_COUNT, DIAMOND_CENTER_LED_COUNT> {
    #[allow(clippy::too_many_lines)]
    fn event(&mut self, event: &Event) -> Result<()> {
        tracing::trace!("UI event: {}", serde_json::to_string(event)?.as_str());
        match event {
            Event::Bootup => {
                self.stop_ring(LEVEL_NOTICE, true);
                self.stop_center(LEVEL_NOTICE, true);
                self.stop_cone(LEVEL_NOTICE, true);
                self.set_ring(
                    LEVEL_BACKGROUND,
                    ring::Idle::<DIAMOND_RING_LED_COUNT>::default(),
                );
                self.operator_pulse.trigger(1., 1., false, false);
            }
            Event::BootComplete { api_mode } => {
                self.sound
                    .queue(sound::Type::Melody(sound::Melody::BootUp))?;
                self.operator_pulse.stop();
                self.operator_idle.api_mode(*api_mode);
            }
            Event::Shutdown { requested } => {
                self.sound
                    .queue(sound::Type::Melody(sound::Melody::PoweringDown))?;
                // overwrite any existing animation by setting notice-level animation
                // as the last animation before shutdown
                self.set_center(
                    LEVEL_NOTICE,
                    center::Alert::<DIAMOND_CENTER_LED_COUNT>::new(
                        if *requested {
                            Argb::DIAMOND_USER_QR_SCAN
                        } else {
                            Argb::DIAMOND_USER_AMBER
                        },
                        vec![0.0, 0.3, 0.45, 0.3, 0.45, 0.45],
                        None,
                        true,
                    ),
                );
                self.set_ring(
                    LEVEL_NOTICE,
                    ring::r#static::Static::<DIAMOND_RING_LED_COUNT>::new(
                        Argb::OFF,
                        None,
                    ),
                );
                self.operator_action
                    .trigger(1.0, Argb::OFF, true, false, true);
            }
            Event::SignupStart => {
                self.capture_sound.reset();
                self.sound
                    .queue(sound::Type::Melody(sound::Melody::StartSignup))?;
                // starting signup sequence
                // animate from left to right (`operator_action`)
                // and then keep first LED on as a background (`operator_signup_phase`)
                self.operator_action.trigger(
                    0.6,
                    Argb::DIAMOND_OPERATOR_DEFAULT,
                    false,
                    true,
                    false,
                );
                self.operator_signup_phase.signup_phase_started();

                // stop all
                self.set_center(
                    LEVEL_BACKGROUND,
                    center::Static::<DIAMOND_CENTER_LED_COUNT>::new(Argb::OFF, None),
                );
                self.stop_ring(LEVEL_FOREGROUND, true);
                self.stop_center(LEVEL_FOREGROUND, true);
                self.stop_center(LEVEL_NOTICE, true);

                self.set_ring(
                    LEVEL_BACKGROUND,
                    ring::r#static::Static::<DIAMOND_RING_LED_COUNT>::new(
                        Argb::DIAMOND_USER_QR_SCAN,
                        None,
                    ),
                );
                self.set_ring(
                    LEVEL_NOTICE,
                    ring::alert::Alert::<DIAMOND_RING_LED_COUNT>::new(
                        Argb::DIAMOND_USER_QR_SCAN,
                        vec![0.0, 0.3, 0.3],
                        None,
                        true,
                    ),
                );
                self.set_cone(
                    LEVEL_NOTICE,
                    ring::alert::Alert::<DIAMOND_CONE_LED_COUNT>::new(
                        Argb::DIAMOND_USER_QR_SCAN,
                        vec![0.0, 0.3, 0.3],
                        None,
                        true,
                    ),
                );
            }
            Event::QrScanStart { schema } => {
                match schema {
                    QrScanSchema::Operator => {
                        self.set_ring(
                            LEVEL_FOREGROUND,
                            ring::r#static::Static::<DIAMOND_RING_LED_COUNT>::new(
                                Argb::DIAMOND_USER_QR_SCAN,
                                None,
                            ),
                        );
                        self.operator_signup_phase.operator_qr_code_ok();
                    }
                    QrScanSchema::Wifi => {
                        self.operator_idle.no_wlan();
                        self.sound.queue(sound::Type::Voice(
                            sound::Voice::ShowWifiHotspotQrCode,
                        ))?;
                    }
                    QrScanSchema::User => {
                        self.operator_signup_phase.user_qr_code_ok();
                        self.set_center(
                            LEVEL_FOREGROUND,
                            center::Static::<DIAMOND_CENTER_LED_COUNT>::new(
                                Argb::DIAMOND_USER_SHROUD,
                                None,
                            ),
                        );
                    }
                };
            }
            Event::QrScanCapture => {
                self.stop_center(LEVEL_FOREGROUND, true);
                self.sound
                    .queue(sound::Type::Melody(sound::Melody::QrCodeCapture))?;
            }
            Event::QrScanCompleted { schema } => {
                self.stop_ring(LEVEL_FOREGROUND, true);
                self.stop_center(LEVEL_FOREGROUND, true);
                // reset ring background to black/off so that it's turned off in next animations
                self.set_ring(
                    LEVEL_BACKGROUND,
                    ring::r#static::Static::<DIAMOND_RING_LED_COUNT>::new(
                        Argb::OFF,
                        None,
                    ),
                );
                match schema {
                    QrScanSchema::Operator => {
                        self.set_ring(
                            LEVEL_FOREGROUND,
                            ring::alert::Alert::<DIAMOND_RING_LED_COUNT>::new(
                                Argb::DIAMOND_USER_QR_SCAN,
                                vec![0.0, 0.5, 0.5],
                                None,
                                true,
                            ),
                        );
                    }
                    QrScanSchema::User => {
                        self.set_center(
                            LEVEL_FOREGROUND,
                            center::Alert::<DIAMOND_CENTER_LED_COUNT>::new(
                                Argb::DIAMOND_USER_SHROUD,
                                vec![0.0, 0.5, 0.5],
                                None,
                                true,
                            ),
                        );
                    }
                    QrScanSchema::Wifi => {}
                }
            }
            Event::QrScanUnexpected { schema, reason } => {
                match reason {
                    QrScanUnexpectedReason::Invalid => {
                        self.sound
                            .queue(sound::Type::Voice(sound::Voice::QrCodeInvalid))?;
                    }
                    QrScanUnexpectedReason::WrongFormat => {
                        self.sound.queue(sound::Type::Voice(
                            sound::Voice::WrongQrCodeFormat,
                        ))?;
                    }
                }
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
                self.sound
                    .queue(sound::Type::Melody(sound::Melody::SoundError))?;
                match schema {
                    QrScanSchema::User | QrScanSchema::Operator => {
                        self.stop_ring(LEVEL_FOREGROUND, true);
                        self.stop_center(LEVEL_FOREGROUND, true);
                        self.set_center(
                            LEVEL_FOREGROUND,
                            center::Static::<DIAMOND_CENTER_LED_COUNT>::new(
                                Argb::OFF,
                                None,
                            ),
                        );
                        self.operator_signup_phase.failure();
                    }
                    QrScanSchema::Wifi => {}
                }
                self.stop_ring(LEVEL_FOREGROUND, true);
            }
            Event::QrScanSuccess { schema } => match schema {
                QrScanSchema::Operator => {
                    self.sound
                        .queue(sound::Type::Melody(sound::Melody::QrLoadSuccess))?;
                    self.operator_signup_phase.operator_qr_captured();
                }
                QrScanSchema::User => {
                    self.sound
                        .queue(sound::Type::Melody(sound::Melody::UserQrLoadSuccess))?;
                    self.operator_signup_phase.user_qr_captured();
                    self.set_center(
                        LEVEL_FOREGROUND,
                        center::Alert::<DIAMOND_CENTER_LED_COUNT>::new(
                            Argb::DIAMOND_USER_SHROUD,
                            vec![0.0, 0.5, 0.5],
                            None,
                            true,
                        ),
                    );
                }
                QrScanSchema::Wifi => {
                    self.sound
                        .queue(sound::Type::Melody(sound::Melody::QrLoadSuccess))?;
                }
            },
            Event::QrScanTimeout { schema } => {
                self.sound
                    .queue(sound::Type::Voice(sound::Voice::Timeout))?;
                match schema {
                    QrScanSchema::User | QrScanSchema::Operator => {
                        self.stop_ring(LEVEL_FOREGROUND, true);
                        self.stop_center(LEVEL_FOREGROUND, true);
                        self.set_center(
                            LEVEL_FOREGROUND,
                            center::Static::<DIAMOND_CENTER_LED_COUNT>::new(
                                Argb::OFF,
                                None,
                            ),
                        );
                        self.operator_signup_phase.failure();
                    }
                    QrScanSchema::Wifi => {}
                }
                self.stop_ring(LEVEL_FOREGROUND, true);
            }
            Event::MagicQrActionCompleted { success } => {
                let melody = if *success {
                    sound::Melody::QrLoadSuccess
                } else {
                    sound::Melody::SoundError
                };
                self.sound.queue(sound::Type::Melody(melody))?;
                // This justs sets the operator LEDs yellow
                // to inform the operator to press the button.
                self.operator_signup_phase.failure();
            }
            Event::NetworkConnectionSuccess => {
                self.sound.queue(sound::Type::Melody(
                    sound::Melody::InternetConnectionSuccessful,
                ))?;
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
                            .downcast_mut::<ring::Progress<DIAMOND_RING_LED_COUNT>>()
                    })
                    .is_none()
                {
                    // in case animation not yet initialized, initialize
                    self.set_ring(
                        LEVEL_NOTICE,
                        ring::Progress::<DIAMOND_RING_LED_COUNT>::new(
                            0.0,
                            None,
                            Argb::DIAMOND_USER_SIGNUP,
                        ),
                    );
                }
                let ring_progress = self
                    .ring_animations_stack
                    .stack
                    .get_mut(&LEVEL_NOTICE)
                    .and_then(|RunningAnimation { animation, .. }| {
                        animation
                            .as_any_mut()
                            .downcast_mut::<ring::Progress<DIAMOND_RING_LED_COUNT>>()
                    });
                if let Some(ring_progress) = ring_progress {
                    ring_progress.set_progress(*progress, None);
                }
            }
            Event::BiometricCaptureOcclusion { occlusion_detected } => {
                if *occlusion_detected {
                    // wave center LEDs
                    self.set_center(
                        LEVEL_NOTICE,
                        center::Wave::<DIAMOND_CENTER_LED_COUNT>::new(
                            Argb::DIAMOND_USER_SHROUD,
                            4.0,
                            0.0,
                            true,
                        ),
                    );
                    self.operator_signup_phase.capture_occlusion_issue();
                } else {
                    self.set_center(
                        LEVEL_NOTICE,
                        center::Static::<DIAMOND_CENTER_LED_COUNT>::new(
                            Argb::DIAMOND_USER_SHROUD,
                            None,
                        ),
                    );
                    self.operator_signup_phase.capture_occlusion_ok();
                }
            }
            Event::BiometricCaptureDistance { in_range } => {
                if *in_range {
                    self.operator_signup_phase.capture_distance_ok();
                    if let Some(melody) = self.capture_sound.peekable().peek() {
                        if self.sound.try_queue(sound::Type::Melody(*melody))? {
                            self.capture_sound.next();
                        }
                    }
                } else {
                    self.operator_signup_phase.capture_distance_issue();
                    self.capture_sound = sound::capture::CaptureLoopSound::default();
                    let _ = self
                        .sound
                        .try_queue(sound::Type::Voice(sound::Voice::Silence));
                }
            }
            Event::BiometricCaptureSuccess => {
                self.sound
                    .queue(sound::Type::Melody(sound::Melody::IrisScanSuccess))?;
                // custom alert animation on ring
                // a bit off for 500ms then on with fade out animation
                // twice: first faster than the other
                self.set_ring(
                    LEVEL_NOTICE,
                    ring::alert::Alert::<DIAMOND_RING_LED_COUNT>::new(
                        Argb::DIAMOND_USER_SIGNUP,
                        vec![0.0, 0.5, 0.75, 0.2, 1.5, 0.2],
                        Some(vec![0.49, 0.4, 0.19, 0.75, 0.2]),
                        false,
                    ),
                );
                self.stop_center(LEVEL_NOTICE, false);
                self.stop_ring(LEVEL_NOTICE, false);

                // preparing animation for biometric pipeline progress
                self.set_ring(
                    LEVEL_FOREGROUND,
                    ring::Progress::<DIAMOND_RING_LED_COUNT>::new(
                        0.0,
                        None,
                        Argb::DIAMOND_USER_SIGNUP,
                    ),
                );

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
                            .downcast_mut::<ring::Progress<DIAMOND_RING_LED_COUNT>>()
                    });
                if let Some(ring_animation) = ring_animation {
                    ring_animation.set_progress(*progress, None);
                } else {
                    tracing::warn!(
                        "BiometricPipelineProgress: ring animation not found"
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
                let progress = self
                    .ring_animations_stack
                    .stack
                    .get_mut(&LEVEL_FOREGROUND)
                    .and_then(|RunningAnimation { animation, .. }| {
                        animation
                            .as_any_mut()
                            .downcast_mut::<ring::Progress<DIAMOND_RING_LED_COUNT>>()
                    });
                if let Some(progress) = progress {
                    progress.set_pulse_angle(PI / 180.0 * 20.0);
                } else {
                    tracing::warn!("StartingEnrollment: ring animation not found");
                }
                self.operator_signup_phase.uploading();
            }
            Event::BiometricPipelineSuccess => {
                let progress = self
                    .ring_animations_stack
                    .stack
                    .get_mut(&LEVEL_FOREGROUND)
                    .and_then(|RunningAnimation { animation, .. }| {
                        animation
                            .as_any_mut()
                            .downcast_mut::<ring::Progress<DIAMOND_RING_LED_COUNT>>()
                    });
                if let Some(progress) = progress {
                    progress.set_progress(2.0, None);
                } else {
                    tracing::warn!("BiometricPipelineSuccess: ring animation not found")
                }

                self.operator_signup_phase.biometric_pipeline_successful();
            }
            Event::SignupFail { reason } => {
                self.sound
                    .queue(sound::Type::Melody(sound::Melody::SoundError))?;
                match reason {
                    SignupFailReason::Timeout => {
                        self.sound
                            .queue(sound::Type::Voice(sound::Voice::Timeout))?;
                    }
                    SignupFailReason::FaceNotFound => {
                        self.sound
                            .queue(sound::Type::Voice(sound::Voice::FaceNotFound))?;
                    }
                    SignupFailReason::Server
                    | SignupFailReason::UploadCustodyImages => {
                        self.sound
                            .queue(sound::Type::Voice(sound::Voice::ServerError))?;
                    }
                    SignupFailReason::Verification => {
                        self.sound.queue(sound::Type::Voice(
                            sound::Voice::VerificationNotSuccessfulPleaseTryAgain,
                        ))?;
                    }
                    SignupFailReason::SoftwareVersionDeprecated => {
                        self.operator_blink.trigger(
                            Argb::DIAMOND_OPERATOR_VERSIONS_DEPRECATED,
                            vec![0.4, 0.4, 0.4, 0.4, 0.4, 0.4],
                        );
                    }
                    SignupFailReason::SoftwareVersionBlocked => {
                        self.operator_blink.trigger(
                            Argb::DIAMOND_OPERATOR_VERSIONS_OUTDATED,
                            vec![0.4, 0.4, 0.4, 0.4, 0.4, 0.4],
                        );
                    }
                    SignupFailReason::Duplicate => {}
                    SignupFailReason::Unknown => {}
                }
                self.operator_signup_phase.failure();

                let progress = self
                    .ring_animations_stack
                    .stack
                    .get_mut(&LEVEL_FOREGROUND)
                    .and_then(|RunningAnimation { animation, .. }| {
                        animation
                            .as_any_mut()
                            .downcast_mut::<ring::Progress<DIAMOND_RING_LED_COUNT>>()
                    });
                if let Some(progress) = progress {
                    progress.set_progress(2.0, None);
                }
                self.stop_ring(LEVEL_FOREGROUND, false);
            }
            Event::SignupSuccess => {
                self.sound
                    .queue(sound::Type::Melody(sound::Melody::SignupSuccess))?;

                self.operator_signup_phase.signup_successful();

                // alert with ring
                self.set_ring(
                    LEVEL_NOTICE,
                    ring::alert::Alert::<DIAMOND_RING_LED_COUNT>::new(
                        Argb::DIAMOND_USER_SIGNUP,
                        vec![0.0, 0.6, 3.6],
                        None,
                        true,
                    ),
                );
            }
            Event::Idle => {
                self.stop_ring(LEVEL_FOREGROUND, true);
                self.stop_center(LEVEL_FOREGROUND, true);
                self.stop_cone(LEVEL_FOREGROUND, true);
                self.stop_ring(LEVEL_NOTICE, false);
                self.stop_center(LEVEL_NOTICE, false);
                self.stop_cone(LEVEL_NOTICE, false);
                self.operator_signup_phase.idle();
            }
            Event::GoodInternet => {
                self.operator_idle.good_internet();
            }
            Event::SlowInternet => {
                self.operator_idle.slow_internet();
            }
            Event::NoInternet => {
                self.operator_idle.no_internet();
            }
            Event::GoodWlan => {
                self.operator_idle.good_wlan();
            }
            Event::SlowWlan => {
                self.operator_idle.slow_wlan();
            }
            Event::NoWlan => {
                self.operator_idle.no_wlan();
            }
            Event::BatteryCapacity { percentage } => {
                self.operator_idle.battery_capacity(*percentage);
            }
            Event::BatteryIsCharging { is_charging } => {
                self.operator_idle.battery_charging(*is_charging);
            }
            Event::Pause => {
                self.paused = true;
            }
            Event::Resume => {
                self.paused = false;
            }
            Event::RecoveryImage => {
                self.sound
                    .queue(sound::Type::Voice(sound::Voice::PleaseDontShutDown))?;
                // check that ring is not already in recovery mode
                if self
                    .ring_animations_stack
                    .stack
                    .get_mut(&LEVEL_NOTICE)
                    .and_then(|RunningAnimation { animation, .. }| {
                        animation
                            .as_any_mut()
                            .downcast_mut::<ring::Spinner<DIAMOND_RING_LED_COUNT>>()
                    })
                    .is_none()
                {
                    self.set_ring(
                        LEVEL_NOTICE,
                        ring::Spinner::<DIAMOND_RING_LED_COUNT>::triple(
                            Argb::DIAMOND_USER_RED,
                        ),
                    );
                }
            }
            Event::NoInternetForSignup => {
                self.sound.queue(sound::Type::Voice(
                    sound::Voice::InternetConnectionTooSlowToPerformSignups,
                ))?;
            }
            Event::SlowInternetForSignup => {
                self.sound.queue(sound::Type::Voice(
                    sound::Voice::InternetConnectionTooSlowSignupsMightTakeLonger,
                ))?;
            }
            Event::SoundVolume { level } => {
                self.sound.set_master_volume(*level);
            }
            Event::SoundLanguage { lang } => {
                let language = lang.clone();
                let sound = self.sound.clone();
                // spawn a new task because we need some async work here
                tokio::task::spawn(async move {
                    let language: Option<&str> = language.as_deref();
                    if let Err(e) = sound.load_sound_files(language, true).await {
                        tracing::error!("Error loading sound files: {:?}", e);
                    }
                });
            }
            Event::SoundTest => {
                self.sound
                    .queue(sound::Type::Melody(sound::Melody::BootUp))?;
            }
        }
        Ok(())
    }

    async fn run(&mut self, interface_tx: &mut Sender<Message>) -> Result<()> {
        let dt = self.timer.get_dt().unwrap_or(0.0);
        self.center_animations_stack.run(&mut self.center_frame, dt);
        if !self.paused {
            interface_tx.try_send(WrappedCenterMessage::from(self.center_frame).0)?;
        }

        self.operator_idle
            .animate(&mut self.operator_frame, dt, false);
        self.operator_signup_phase
            .animate(&mut self.operator_frame, dt, false);
        self.operator_blink
            .animate(&mut self.operator_frame, dt, false);
        self.operator_pulse
            .animate(&mut self.operator_frame, dt, false);
        self.operator_action
            .animate(&mut self.operator_frame, dt, false);
        if !self.paused {
            // 2ms sleep to make sure UART communication is over
            time::sleep(Duration::from_millis(2)).await;
            interface_tx
                .try_send(WrappedOperatorMessage::from(self.operator_frame).0)?;
        }

        self.ring_animations_stack.run(&mut self.ring_frame, dt);
        if !self.paused {
            time::sleep(Duration::from_millis(2)).await;
            interface_tx.try_send(WrappedRingMessage::from(self.ring_frame).0)?;
        }
        if let Some(animation) = &mut self.cone_animations_stack {
            if let Some(frame) = &mut self.cone_frame {
                animation.run(frame, dt);
                if !self.paused {
                    time::sleep(Duration::from_millis(2)).await;
                    interface_tx.try_send(WrappedConeMessage::from(*frame).0)?;
                }
            }
        }
        Ok(())
    }
}
