use async_trait::async_trait;
use eyre::Result;
use futures::channel::mpsc::Sender;
use futures::future::Either;
use futures::{future, StreamExt};
use orb_messages::mcu_main::mcu_message::Message;
use orb_messages::mcu_main::{jetson_to_mcu, JetsonToMcu};
use orb_rgb::Argb;
use pid::{InstantTimer, Timer};
use std::f64::consts::PI;
use std::time::Duration;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::time;
use tokio_stream::wrappers::{IntervalStream, UnboundedReceiverStream};

use crate::engine::animations::alert::BlinkDurations;
use crate::engine::{
    animations, operator, Animation, AnimationsStack, CenterFrame, ConeFrame, Event,
    EventHandler, OperatorFrame, OrbType, QrScanSchema, QrScanUnexpectedReason,
    RingFrame, Runner, RunningAnimation, SignupFailReason, Transition,
    DIAMOND_CENTER_LED_COUNT, DIAMOND_CONE_LED_COUNT, DIAMOND_RING_LED_COUNT,
    LED_ENGINE_FPS, LEVEL_BACKGROUND, LEVEL_FOREGROUND, LEVEL_NOTICE,
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

/// Convert a `RingFrame` into a `WrappedRingMessage`, which wraps the protobuf message
/// to be sent to the MCU.
///
/// On Diamond, the outer ring light isn't diffused the same way
/// at the top than the bottom and brightness looks different.
/// to compensate for that, we need to modify the brightness for each LED
/// in the ring with the equation below.
impl From<RingFrame<DIAMOND_RING_LED_COUNT>> for WrappedRingMessage {
    fn from(value: RingFrame<DIAMOND_RING_LED_COUNT>) -> Self {
        WrappedRingMessage(Message::JMessage(
            JetsonToMcu {
                ack_number: 0,
                payload: Some(jetson_to_mcu::Payload::RingLedsSequence(
                    orb_messages::mcu_main::UserRingLeDsSequence {
                        data_format: Some(
                            orb_messages::mcu_main::user_ring_le_ds_sequence::DataFormat::Argb32Uncompressed(
                                value.iter().rev().enumerate().flat_map(|(i, &Argb(a, r, g, b))| {
                                    // adapt brightness depending on the position in the ring
                                    // equation given by the hardware team
                                    // https://linear.app/worldcoin/issue/ORBP-146/ui-adjust-brightness-depending-on-ring-location
                                    let angle = ((i % (DIAMOND_RING_LED_COUNT / 2)) * 180 / (DIAMOND_RING_LED_COUNT / 2)) as f64;
                                    let b_factor = 1.5
                                        + 9.649 * 10f64.powf(-8.0) * angle.powf(3.0)
                                        - 2.784 * 10f64.powf(-5.0) * angle.powf(2.0)
                                        - 1.225 * 10f64.powf(-3.0) * angle;
                                    [a.unwrap_or(0_u8), (r as f64 * b_factor) as u8, (g as f64 * b_factor) as u8, (b as f64 * b_factor) as u8]
                                }).collect(),
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
            is_api_mode: false,
            paused: false,
            gimbal: None,
        }
    }

    fn set_ring(
        &mut self,
        level: u8,
        animation: impl Animation<Frame = RingFrame<DIAMOND_RING_LED_COUNT>>,
    ) {
        self.ring_animations_stack.set(level, Box::new(animation));
    }

    #[expect(dead_code)]
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

    fn stop_ring(&mut self, level: u8, transition: Transition) {
        self.ring_animations_stack.stop(level, transition);
    }

    #[expect(dead_code)]
    fn stop_cone(&mut self, level: u8, transition: Transition) {
        if let Some(animations) = &mut self.cone_animations_stack {
            animations.stop(level, transition);
        }
    }

    fn stop_center(&mut self, level: u8, transition: Transition) {
        self.center_animations_stack.stop(level, transition);
    }
}

#[async_trait]
impl EventHandler for Runner<DIAMOND_RING_LED_COUNT, DIAMOND_CENTER_LED_COUNT> {
    #[allow(clippy::too_many_lines)]
    fn event(&mut self, event: &Event) -> Result<()> {
        tracing::debug!("UI event: {}", serde_json::to_string(event)?.as_str());
        match event {
            Event::Bootup => {
                self.stop_ring(LEVEL_NOTICE, Transition::ForceStop);
                self.stop_center(LEVEL_NOTICE, Transition::ForceStop);
                self.set_ring(
                    LEVEL_BACKGROUND,
                    animations::Idle::<DIAMOND_RING_LED_COUNT>::default(),
                );
                self.operator_pulse.trigger(1., 1., false, false);
            }
            Event::NetworkConnectionSuccess => {
                self.sound.queue(
                    sound::Type::Melody(sound::Melody::InternetConnectionSuccessful),
                    Duration::ZERO,
                )?;
            }
            Event::BootComplete { api_mode } => {
                self.sound.queue(
                    sound::Type::Melody(sound::Melody::BootUp),
                    Duration::ZERO,
                )?;
                self.operator_pulse.stop(Transition::PlayOnce)?;
                self.operator_idle.api_mode(*api_mode);
                self.is_api_mode = *api_mode;

                // make sure we set the background to off
                self.set_center(
                    LEVEL_BACKGROUND,
                    animations::Static::<DIAMOND_CENTER_LED_COUNT>::new(
                        Argb::OFF,
                        None,
                    ),
                );
                self.set_ring(
                    LEVEL_BACKGROUND,
                    animations::Static::<DIAMOND_RING_LED_COUNT>::new(Argb::OFF, None),
                );
            }
            Event::Shutdown { requested: _ } => {
                self.sound.queue(
                    sound::Type::Melody(sound::Melody::PoweringDown),
                    Duration::ZERO,
                )?;
                self.set_ring(
                    LEVEL_NOTICE,
                    animations::Static::<DIAMOND_RING_LED_COUNT>::new(Argb::OFF, None),
                );
                self.operator_action
                    .trigger(1.0, Argb::OFF, true, false, true);
            }
            Event::SignupStartOperator => {
                self.capture_sound.reset();
                self.sound.queue(
                    sound::Type::Melody(sound::Melody::StartSignup),
                    Duration::ZERO,
                )?;
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
                    animations::Static::<DIAMOND_CENTER_LED_COUNT>::new(
                        Argb::OFF,
                        None,
                    ),
                );
                self.stop_center(LEVEL_FOREGROUND, Transition::ForceStop);
                self.stop_center(LEVEL_NOTICE, Transition::ForceStop);

                self.set_ring(
                    LEVEL_BACKGROUND,
                    animations::Static::<DIAMOND_RING_LED_COUNT>::new(Argb::OFF, None),
                );
            }
            Event::QrScanStart { schema } => {
                self.stop_center(LEVEL_FOREGROUND, Transition::ForceStop);
                match schema {
                    QrScanSchema::OperatorSelfServe => {
                        self.operator_signup_phase.signup_phase_started();
                        self.set_ring(
                            LEVEL_FOREGROUND,
                            animations::SimpleSpinner::new(
                                Argb::DIAMOND_RING_OPERATOR_QR_SCAN_SPINNER,
                                Some(Argb::DIAMOND_RING_OPERATOR_QR_SCAN),
                            )
                            .fade_in(1.5),
                        );
                        self.operator_signup_phase.operator_qr_code_ok();
                    }
                    QrScanSchema::Operator => {
                        self.operator_signup_phase.signup_phase_started();
                        self.set_ring(
                            LEVEL_FOREGROUND,
                            animations::SimpleSpinner::new(
                                Argb::DIAMOND_RING_OPERATOR_QR_SCAN_SPINNER_OPERATOR_BASED,
                                Some(Argb::OFF),
                            )
                            .fade_in(1.5),
                        );
                        self.operator_signup_phase.operator_qr_code_ok();
                    }
                    QrScanSchema::Wifi => {
                        self.operator_idle.no_wlan();
                        self.sound.queue(
                            sound::Type::Voice(sound::Voice::ShowWifiHotspotQrCode),
                            Duration::ZERO,
                        )?;
                    }
                    QrScanSchema::User => {
                        self.operator_signup_phase.user_qr_code_ok();
                        self.set_ring(
                            LEVEL_FOREGROUND,
                            animations::SimpleSpinner::new(
                                Argb::DIAMOND_RING_USER_QR_SCAN_SPINNER,
                                Some(Argb::DIAMOND_RING_USER_QR_SCAN),
                            )
                            .fade_in(1.5),
                        );
                    }
                };
            }
            Event::QrScanCapture => {
                self.sound.queue(
                    sound::Type::Melody(sound::Melody::QrCodeCapture),
                    Duration::ZERO,
                )?;
            }
            Event::QrScanCompleted { schema: _ } => {
                self.stop_center(LEVEL_FOREGROUND, Transition::ForceStop);
                // reset ring background to black/off so that it's turned off in next animations
                self.set_ring(
                    LEVEL_BACKGROUND,
                    animations::Static::<DIAMOND_RING_LED_COUNT>::new(Argb::OFF, None),
                );
            }
            Event::QrScanUnexpected { schema, reason } => {
                self.set_ring(
                    LEVEL_NOTICE,
                    animations::Alert::<DIAMOND_RING_LED_COUNT>::new(
                        Argb::DIAMOND_RING_ERROR_SALMON,
                        BlinkDurations::from(vec![0.0, 2.0, 4.0]),
                        Some(vec![1.0, 1.5]),
                        true,
                    ),
                );
                match reason {
                    QrScanUnexpectedReason::Invalid => {
                        self.sound.queue(
                            sound::Type::Voice(sound::Voice::QrCodeInvalid),
                            Duration::ZERO,
                        )?;
                    }
                    QrScanUnexpectedReason::WrongFormat => {
                        self.sound.queue(
                            sound::Type::Voice(sound::Voice::WrongQrCodeFormat),
                            Duration::ZERO,
                        )?;
                    }
                }
                match schema {
                    QrScanSchema::User => {
                        self.operator_signup_phase.user_qr_code_issue();
                    }
                    QrScanSchema::Operator | QrScanSchema::OperatorSelfServe => {
                        self.operator_signup_phase.operator_qr_code_issue();
                    }
                    QrScanSchema::Wifi => {}
                }
            }
            Event::QrScanFail { schema } => {
                self.sound.queue(
                    sound::Type::Melody(sound::Melody::SoundError),
                    Duration::ZERO,
                )?;
                match schema {
                    QrScanSchema::User
                    | QrScanSchema::Operator
                    | QrScanSchema::OperatorSelfServe => {
                        self.operator_signup_phase.failure();
                        self.set_ring(
                            LEVEL_NOTICE,
                            animations::Alert::<DIAMOND_RING_LED_COUNT>::new(
                                Argb::DIAMOND_RING_ERROR_SALMON,
                                BlinkDurations::from(vec![0.0, 2.0, 4.0]),
                                Some(vec![1.0, 1.5]),
                                true,
                            ),
                        );
                    }
                    QrScanSchema::Wifi => {}
                }
            }
            Event::QrScanSuccess { schema } => match schema {
                QrScanSchema::Operator | QrScanSchema::OperatorSelfServe => {
                    self.sound.queue(
                        sound::Type::Melody(sound::Melody::QrLoadSuccess),
                        Duration::ZERO,
                    )?;
                    self.operator_signup_phase.operator_qr_captured();
                }
                QrScanSchema::User => {
                    self.sound.queue(
                        sound::Type::Melody(sound::Melody::UserQrLoadSuccess),
                        Duration::ZERO,
                    )?;
                    self.operator_signup_phase.user_qr_captured();
                    self.set_ring(
                        LEVEL_FOREGROUND,
                        animations::SimpleSpinner::new(
                            Argb::DIAMOND_RING_USER_QR_SCAN_SPINNER,
                            Some(Argb::DIAMOND_RING_USER_QR_SCAN),
                        )
                        .speed(2.0 * PI / 7.0), // 7 seconds per turn
                    );
                }
                QrScanSchema::Wifi => {
                    self.sound.queue(
                        sound::Type::Melody(sound::Melody::QrLoadSuccess),
                        Duration::ZERO,
                    )?;
                }
            },
            Event::QrScanTimeout { schema } => {
                self.sound
                    .queue(sound::Type::Voice(sound::Voice::Timeout), Duration::ZERO)?;
                match schema {
                    QrScanSchema::User
                    | QrScanSchema::Operator
                    | QrScanSchema::OperatorSelfServe => {
                        self.operator_signup_phase.failure();

                        // show error animation
                        self.stop_ring(LEVEL_FOREGROUND, Transition::ForceStop);
                        self.set_ring(
                            LEVEL_NOTICE,
                            animations::Alert::<DIAMOND_RING_LED_COUNT>::new(
                                Argb::DIAMOND_RING_ERROR_SALMON,
                                BlinkDurations::from(vec![0.0, 2.0, 4.0]),
                                Some(vec![1.0, 1.5]),
                                true,
                            ),
                        );
                    }
                    QrScanSchema::Wifi => {
                        self.stop_ring(LEVEL_FOREGROUND, Transition::FadeOut(1.0));
                    }
                }
            }
            Event::MagicQrActionCompleted { success } => {
                let melody = if *success {
                    sound::Melody::QrLoadSuccess
                } else {
                    sound::Melody::SoundError
                };
                self.sound
                    .queue(sound::Type::Melody(melody), Duration::ZERO)?;
                // This justs sets the operator LEDs yellow
                // to inform the operator to press the button.
                self.operator_signup_phase.failure();
            }
            Event::SignupStart => {
                self.capture_sound.reset();
                // if not self-serve, the animations to transition
                // to biometric capture are already set in `QrScanSuccess`
                self.sound.queue(
                    sound::Type::Melody(sound::Melody::UserStartCapture),
                    Duration::ZERO,
                )?;
                self.stop_ring(LEVEL_FOREGROUND, Transition::ForceStop);
                // 2-blink alert + fade-out
                self.set_ring(
                    LEVEL_BACKGROUND,
                    animations::Static::<DIAMOND_RING_LED_COUNT>::new(Argb::OFF, None),
                );
                self.set_ring(
                    LEVEL_FOREGROUND,
                    animations::Alert::<DIAMOND_RING_LED_COUNT>::new(
                        Argb::DIAMOND_RING_USER_CAPTURE,
                        BlinkDurations::from(vec![0.0, 0.4, 0.2, 0.4]),
                        Some(vec![0.2, 0.2, 0.01]),
                        true,
                    ),
                );
                // pulsing wave animation displayed
                // while we wait for the user to be in position
                self.set_center(
                    LEVEL_FOREGROUND,
                    animations::Wave::<DIAMOND_CENTER_LED_COUNT>::new(
                        Argb::DIAMOND_SHROUD_SUMMON_USER_AMBER,
                        3.0,
                        0.0,
                        true,
                    )
                    .with_delay(1.5),
                );
            }
            Event::BiometricCaptureHalfObjectivesCompleted => {
                // do nothing
            }
            Event::BiometricCaptureAllObjectivesCompleted => {
                self.operator_signup_phase.irises_captured();
            }
            Event::BiometricCaptureProgress { progress } => {
                // set progress but wait for shroud to finish breathing
                let shroud_breathing = self
                    .center_animations_stack
                    .stack
                    .get_mut(&LEVEL_FOREGROUND)
                    .and_then(|RunningAnimation { animation, .. }| {
                        animation
                            .as_any_mut()
                            .downcast_mut::<animations::Wave<DIAMOND_CENTER_LED_COUNT>>(
                            )
                    })
                    .is_some();
                if !shroud_breathing {
                    if self
                        .ring_animations_stack
                        .stack
                        .get_mut(&LEVEL_NOTICE)
                        .and_then(|RunningAnimation { animation, .. }| {
                            animation
                                .as_any_mut()
                                .downcast_mut::<animations::Progress<DIAMOND_RING_LED_COUNT>>()
                        })
                        .is_none() || *progress <= 0.01
                    {
                        // in case animation not yet initialized, initialize
                        self.set_ring(
                            LEVEL_NOTICE,
                            animations::Progress::<DIAMOND_RING_LED_COUNT>::new(
                                0.0,
                                None,
                                Argb::DIAMOND_RING_USER_CAPTURE,
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
                                .downcast_mut::<animations::Progress<DIAMOND_RING_LED_COUNT>>()
                        });
                    if let Some(ring_progress) = ring_progress {
                        ring_progress.set_progress(*progress, None);
                    }
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
                // show correct user position to operator with operator leds
                if *in_range {
                    self.operator_signup_phase.capture_distance_ok();
                } else {
                    self.operator_signup_phase.capture_distance_issue();
                }

                // show correct position to user by playing sounds but
                // only once shroud stops breathing
                let shround_breathing = self
                    .center_animations_stack
                    .stack
                    .get_mut(&LEVEL_FOREGROUND)
                    .and_then(|RunningAnimation { animation, .. }| {
                        animation
                            .as_any_mut()
                            .downcast_mut::<animations::Wave<DIAMOND_CENTER_LED_COUNT>>(
                            )
                    })
                    .is_some();
                if shround_breathing && *in_range {
                    // stop any ongoing breathing animation and transition to static
                    self.set_center(
                        LEVEL_FOREGROUND,
                        animations::Static::<DIAMOND_CENTER_LED_COUNT>::new(
                            Argb::DIAMOND_SHROUD_SUMMON_USER_AMBER,
                            None,
                        )
                        .fade_in(1.5),
                    );
                } else if *in_range {
                    if let Some(melody) = self.capture_sound.peekable().peek() {
                        if self.sound.try_queue(sound::Type::Melody(*melody))? {
                            self.capture_sound.next();
                        }
                    }
                } else {
                    self.capture_sound = sound::capture::CaptureLoopSound::default();
                    let _ = self
                        .sound
                        .try_queue(sound::Type::Voice(sound::Voice::Silence));
                }
            }
            Event::BiometricCaptureSuccess => {
                // fade out duration + sound delay
                // delaying the sound allows resynchronizing in case another
                // sound is played at the same time, as the delay start
                // when the sound is queued.
                let fade_out_duration = 0.7;
                self.sound.queue(
                    sound::Type::Melody(sound::Melody::IrisScanSuccess),
                    Duration::from_millis((fade_out_duration * 1000.0) as u64),
                )?;
                // custom alert animation on ring
                // a bit off for 500ms then on with fade out animation
                // twice: first faster than the other
                self.stop_center(
                    LEVEL_FOREGROUND,
                    Transition::FadeOut(fade_out_duration),
                );
                // in case nothing is running on center, make sure we set the background to off
                self.set_center(
                    LEVEL_BACKGROUND,
                    animations::Static::<DIAMOND_CENTER_LED_COUNT>::new(
                        Argb::OFF,
                        None,
                    ),
                );
                self.set_ring(
                    LEVEL_NOTICE,
                    animations::Alert::<DIAMOND_RING_LED_COUNT>::new(
                        Argb::DIAMOND_RING_USER_CAPTURE,
                        BlinkDurations::from(vec![
                            0.0,
                            fade_out_duration,
                            0.5,
                            0.75,
                            0.5,
                            1.5,
                            0.5,
                            3.0,
                            0.2,
                        ]),
                        Some(vec![0.1, 0.4, 0.4, 0.2, 0.75, 0.2, 0.2, 1.0]),
                        false,
                    ),
                );

                self.operator_signup_phase.iris_scan_complete();
            }
            Event::BiometricPipelineProgress { progress } => {
                if *progress <= 0.01 {
                    self.stop_ring(LEVEL_FOREGROUND, Transition::ForceStop);
                    self.set_ring(
                        LEVEL_FOREGROUND,
                        animations::Progress::<DIAMOND_RING_LED_COUNT>::new(
                            0.0,
                            None,
                            Argb::DIAMOND_RING_USER_CAPTURE,
                        ),
                    );
                } else if let Some(ring_animation) = self
                    .ring_animations_stack
                    .stack
                    .get_mut(&LEVEL_FOREGROUND)
                    .and_then(|RunningAnimation { animation, .. }| {
                        animation
                            .as_any_mut()
                            .downcast_mut::<animations::Progress<DIAMOND_RING_LED_COUNT>>()
                    }) {
                    ring_animation.set_progress(*progress, None);
                } else {
                    self.set_ring(
                        LEVEL_FOREGROUND,
                        animations::Progress::<DIAMOND_RING_LED_COUNT>::new(
                            0.0,
                            None,
                            Argb::DIAMOND_RING_USER_CAPTURE,
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
                let progress = self
                    .ring_animations_stack
                    .stack
                    .get_mut(&LEVEL_FOREGROUND)
                    .and_then(|RunningAnimation { animation, .. }| {
                        animation
                            .as_any_mut()
                            .downcast_mut::<animations::Progress<DIAMOND_RING_LED_COUNT>>()
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
                            .downcast_mut::<animations::Progress<DIAMOND_RING_LED_COUNT>>()
                    });
                if let Some(progress) = progress {
                    progress.set_progress(2.0, None);
                } else {
                    tracing::warn!("BiometricPipelineSuccess: ring animation not found")
                }

                self.operator_signup_phase.biometric_pipeline_successful();
            }
            Event::SignupFail { reason } => {
                // replace background
                self.set_ring(
                    LEVEL_BACKGROUND,
                    animations::Static::<DIAMOND_RING_LED_COUNT>::new(Argb::OFF, None),
                );
                self.sound.queue(
                    sound::Type::Melody(sound::Melody::SoundError),
                    Duration::from_millis(2000),
                )?;
                match reason {
                    SignupFailReason::Timeout => {
                        self.sound.queue(
                            sound::Type::Voice(sound::Voice::Timeout),
                            Duration::ZERO,
                        )?;
                    }
                    SignupFailReason::FaceNotFound => {
                        self.sound.queue(
                            sound::Type::Voice(sound::Voice::FaceNotFound),
                            Duration::ZERO,
                        )?;
                    }
                    SignupFailReason::Server
                    | SignupFailReason::UploadCustodyImages => {
                        self.sound.queue(
                            sound::Type::Voice(sound::Voice::ServerError),
                            Duration::ZERO,
                        )?;
                    }
                    SignupFailReason::Verification => {
                        self.sound.queue(
                            sound::Type::Voice(
                                sound::Voice::VerificationNotSuccessfulPleaseTryAgain,
                            ),
                            Duration::ZERO,
                        )?;
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

                // turn off center
                self.stop_center(LEVEL_FOREGROUND, Transition::ForceStop);
                self.stop_center(LEVEL_NOTICE, Transition::ForceStop);

                // close biometric capture progress
                if let Some(progress) = self
                    .ring_animations_stack
                    .stack
                    .get_mut(&LEVEL_NOTICE)
                    .and_then(|RunningAnimation { animation, .. }| {
                        animation
                            .as_any_mut()
                            .downcast_mut::<animations::Progress<DIAMOND_RING_LED_COUNT>>()
                    })
                {
                    progress.set_progress(2.0, None);
                }

                // close biometric pipeline progress
                if let Some(progress) = self
                    .ring_animations_stack
                    .stack
                    .get_mut(&LEVEL_FOREGROUND)
                    .and_then(|RunningAnimation { animation, .. }| {
                        animation
                            .as_any_mut()
                            .downcast_mut::<animations::Progress<DIAMOND_RING_LED_COUNT>>()
                    })
                {
                    progress.set_progress(2.0, None);
                }
                self.stop_ring(LEVEL_FOREGROUND, Transition::ForceStop);

                // show error animation
                self.set_ring(
                    LEVEL_NOTICE,
                    animations::Alert::<DIAMOND_RING_LED_COUNT>::new(
                        Argb::DIAMOND_RING_ERROR_SALMON,
                        BlinkDurations::from(vec![0.0, 2.0, 4.0]),
                        Some(vec![1.0, 1.5]),
                        true,
                    ),
                );
            }
            Event::SignupSuccess => {
                self.sound.queue(
                    sound::Type::Melody(sound::Melody::SignupSuccess),
                    Duration::ZERO,
                )?;

                self.operator_signup_phase.signup_successful();

                // replace background
                self.set_ring(
                    LEVEL_BACKGROUND,
                    animations::Static::<DIAMOND_RING_LED_COUNT>::new(Argb::OFF, None),
                );
                // alert with ring
                self.set_ring(
                    LEVEL_NOTICE,
                    animations::Alert::<DIAMOND_RING_LED_COUNT>::new(
                        Argb::DIAMOND_RING_USER_CAPTURE,
                        BlinkDurations::from(vec![0.0, 0.6, 3.6]),
                        None,
                        false,
                    ),
                );
            }
            Event::Idle => {
                self.stop_ring(LEVEL_FOREGROUND, Transition::ForceStop);
                self.stop_center(LEVEL_FOREGROUND, Transition::ForceStop);
                self.stop_ring(LEVEL_NOTICE, Transition::FadeOut(0.5));
                self.stop_center(LEVEL_NOTICE, Transition::FadeOut(0.5));

                self.operator_signup_phase.idle();
                self.set_center(
                    LEVEL_BACKGROUND,
                    animations::Static::<DIAMOND_CENTER_LED_COUNT>::new(
                        Argb::OFF,
                        None,
                    ),
                );
                self.set_ring(
                    LEVEL_FOREGROUND,
                    animations::Static::<DIAMOND_RING_LED_COUNT>::new(
                        Argb::DIAMOND_RING_USER_QR_SCAN,
                        None,
                    )
                    .fade_in(1.5),
                );
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
                self.sound.queue(
                    sound::Type::Voice(sound::Voice::PleaseDontShutDown),
                    Duration::ZERO,
                )?;
                // check that ring is not already in recovery mode
                if self
                    .ring_animations_stack
                    .stack
                    .get_mut(&LEVEL_NOTICE)
                    .and_then(|RunningAnimation { animation, .. }| {
                        animation
                            .as_any_mut()
                            .downcast_mut::<animations::Spinner<DIAMOND_RING_LED_COUNT>>()
                    })
                    .is_none()
                {
                    self.set_ring(
                        LEVEL_NOTICE,
                        animations::Spinner::<DIAMOND_RING_LED_COUNT>::triple(
                            Argb::DIAMOND_SHROUD_SUMMON_USER_AMBER,
                            None,
                        ),
                    );
                }
            }
            Event::NoInternetForSignup => {
                self.sound.queue(
                    sound::Type::Voice(
                        sound::Voice::InternetConnectionTooSlowToPerformSignups,
                    ),
                    Duration::ZERO,
                )?;
            }
            Event::SlowInternetForSignup => {
                self.sound.queue(
                    sound::Type::Voice(
                        sound::Voice::InternetConnectionTooSlowSignupsMightTakeLonger,
                    ),
                    Duration::ZERO,
                )?;
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
                self.sound.queue(
                    sound::Type::Melody(sound::Melody::BootUp),
                    Duration::ZERO,
                )?;
            }
            Event::Gimbal { x, y } => {
                self.gimbal = Some((*x, *y));
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

        if let Some((x, y)) = self.gimbal {
            interface_tx.try_send(Message::JMessage(JetsonToMcu {
                ack_number: 0,
                payload: Some(jetson_to_mcu::Payload::MirrorAngle(
                    orb_messages::mcu_main::MirrorAngle {
                        horizontal_angle: 0,
                        vertical_angle: 0,
                        phi_angle_millidegrees: x,
                        theta_angle_millidegrees: y,
                        angle_type: orb_messages::mcu_main::MirrorAngleType::PhiTheta
                            as i32,
                    },
                )),
            }))?;

            // send only once
            self.gimbal = None;
        }

        // one last update of the UI has been performed since api_mode has been set,
        // (to set the api_mode UI state), so we can now pause the engine
        if self.is_api_mode && !self.paused {
            self.paused = true;
            tracing::info!("UI paused in API mode");
        } else if !self.is_api_mode && self.paused {
            self.paused = false;
            tracing::info!("UI resumed from API mode");
        }

        Ok(())
    }
}
