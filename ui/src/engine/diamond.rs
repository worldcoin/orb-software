use async_trait::async_trait;
use eyre::Result;
use futures::channel::mpsc::Sender;
use futures::future::Either;
use futures::{future, StreamExt};
use orb_messages::main::{jetson_to_mcu, JetsonToMcu};
use orb_messages::mcu_message::Message;
use orb_rgb::Argb;
use pid::{InstantTimer, Timer};
use std::time::Duration;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::time;
use tokio_stream::wrappers::{IntervalStream, UnboundedReceiverStream};

use crate::engine::animations::alert::BlinkDurations;
use crate::engine::{
    animations, operator, Animation, AnimationsStack, CenterFrame, ConeFrame, Event,
    EventHandler, OperatingMode, OperatorFrame, OrbType, QrScanSchema,
    QrScanUnexpectedReason, RingFrame, Runner, RunningAnimation, SignupFailReason,
    Transition, UiMode, UiState, DIAMOND_CENTER_LED_COUNT, DIAMOND_CONE_LED_COUNT,
    DIAMOND_RING_LED_COUNT, LED_ENGINE_FPS, LEVEL_BACKGROUND, LEVEL_FOREGROUND,
    LEVEL_NOTICE,
};
use crate::sound;
use crate::sound::Player;

use super::animations::alert_v2::SquarePulseTrain;
use super::CriticalState;

/// Ring color for the fake-progress loading bar.
/// Controlled by a single feature flag at build time.
/// Priority (first match wins): ring-white > green-v1 > green-v2 > green-v3 > green-v4 > default.
const fn fake_progress_ring_color() -> Argb {
    if cfg!(feature = "ring-white") {
        Argb(Some(10), 90, 90, 90)
    } else if cfg!(feature = "green-v1") {
        Argb(Some(10), 0, 200, 0)
    } else if cfg!(feature = "green-v2") {
        Argb(Some(10), 120, 210, 0)
    } else if cfg!(feature = "green-v3") {
        Argb(Some(10), 0, 200, 100)
    } else if cfg!(feature = "green-v4") {
        Argb(Some(10), 160, 240, 100)
    } else {
        Argb(Some(10), 0, 100, 0)
    }
}

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
                    orb_messages::main::UserCenterLeDsSequence {
                        data_format: Some(
                            orb_messages::main::user_center_le_ds_sequence::DataFormat::Argb32Uncompressed(
                                // turn off one LED every 2 LEDs to decrease general brightness
                                value.iter().enumerate().flat_map(|(i, &Argb(a, r, g, b))| {
                                    if i % 2 == 0 {
                                        [a.unwrap_or(0_u8), r, g, b]
                                    } else {
                                        [0, 0, 0, 0]
                                    }
                                }).collect(),
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
                    orb_messages::main::UserRingLeDsSequence {
                        data_format: Some(
                            orb_messages::main::user_ring_le_ds_sequence::DataFormat::Argb32Uncompressed(
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
                    orb_messages::main::ConeLeDsSequence {
                        data_format: Some(
                            orb_messages::main::cone_le_ds_sequence::DataFormat::Argb32Uncompressed(
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
                    orb_messages::main::DistributorLeDsSequence {
                        data_format: Some(
                            orb_messages::main::distributor_le_ds_sequence::DataFormat::Argb32Uncompressed(
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
            return Err(eyre::eyre!("Failed to initialize sound: {:?}", e));
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
            capture_succeeded: false,
            occlusion_active: false,
            occlusion_sound_last_played: None,
            capture_distance_in_range: true,
            state: UiState::Booting,
            gimbal: None,
            operating_mode: OperatingMode::default(),
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

    fn set_occlusion_center_animation(&mut self) {
        self.set_center(
            LEVEL_NOTICE,
            animations::sine_blend::SineBlend::<DIAMOND_CENTER_LED_COUNT>::new(
                Argb::DIAMOND_CENTER_USER_QR_SCAN_SUCCESS_COOL_WHITE,
                Argb::OFF,
                2.0,
                0.0,
            ),
        );
    }

    fn biometric_capture_success(&mut self) -> Result<()> {
        self.capture_succeeded = true;
        let master_volume = self.sound.volume();
        self.sound.set_master_volume((master_volume * 2).min(100));
        self.sound.queue(
            sound::Type::Melody(sound::Melody::SignupSuccess),
            Duration::ZERO,
        )?;
        self.sound.set_master_volume(master_volume);

        self.stop_ring(LEVEL_FOREGROUND, Transition::ForceStop);
        self.stop_ring(LEVEL_NOTICE, Transition::ForceStop);
        self.set_ring(
            LEVEL_FOREGROUND,
            animations::sine_blend::SineBlend::<DIAMOND_RING_LED_COUNT>::new(
                Argb::DIAMOND_RING_BIOMETRIC_CAPTURE_SUCCESS_GREEN,
                Argb::OFF,
                4.0,
                0.0,
            ),
        );
        self.stop_center(LEVEL_FOREGROUND, Transition::ForceStop);
        self.set_center(
            LEVEL_FOREGROUND,
            animations::Static::<DIAMOND_CENTER_LED_COUNT>::new(
                Argb::DIAMOND_CENTER_BIOMETRIC_CAPTURE_SUCCESS_GREEN,
                None,
            ),
        );
        self.set_center(
            LEVEL_NOTICE,
            animations::Alert::<DIAMOND_CENTER_LED_COUNT>::new(
                Argb::DIAMOND_CENTER_BIOMETRIC_CAPTURE_SUCCESS_GREEN,
                BlinkDurations::from(vec![0.0, 0.3, 0.2, 0.3, 0.2, 0.3]),
                None,
                true,
            )?,
        );

        // move mirror to flat (phi=0, theta=0)
        self.gimbal = Some((0, 0));

        Ok(())
    }

    fn play_signup_fail_ux(&mut self, sound: Option<sound::Type>) -> Result<()> {
        self.sound.queue(
            sound::Type::Melody(sound::Melody::SoundError),
            Duration::from_millis(2000),
        )?;

        if let Some(sound) = sound {
            self.sound.queue(sound, Duration::ZERO)?;
        }

        // turn off center
        self.stop_center(LEVEL_FOREGROUND, Transition::ForceStop);
        self.stop_center(LEVEL_NOTICE, Transition::ForceStop);

        // ring, run error animation at NOTICE level, off for the rest.
        self.set_ring(
            LEVEL_BACKGROUND,
            animations::Static::<DIAMOND_RING_LED_COUNT>::new(Argb::OFF, None),
        );
        self.stop_ring(LEVEL_FOREGROUND, Transition::ForceStop);
        self.stop_ring(LEVEL_NOTICE, Transition::ForceStop);
        self.set_center(
            LEVEL_NOTICE,
            animations::Alert::<DIAMOND_CENTER_LED_COUNT>::new(
                Argb::DIAMOND_RING_ERROR_SALMON,
                BlinkDurations::from(vec![0.0, 1.5, 4.0]),
                Some(vec![0.5, 1.5]),
                true,
            )?,
        );

        Ok(())
    }
}

#[async_trait]
impl EventHandler for Runner<DIAMOND_RING_LED_COUNT, DIAMOND_CENTER_LED_COUNT> {
    #[allow(clippy::too_many_lines)]
    fn event(&mut self, event: &Event) -> Result<()> {
        tracing::debug!("event: {:?}", event);
        match event {
            Event::Bootup => {
                self.stop_ring(LEVEL_NOTICE, Transition::ForceStop);
                self.stop_center(LEVEL_NOTICE, Transition::ForceStop);
                self.set_ring(
                    LEVEL_BACKGROUND,
                    animations::Idle::<DIAMOND_RING_LED_COUNT>::default(),
                );
            }
            Event::InternetConnecting => {
                self.set_center(
                    LEVEL_BACKGROUND,
                    animations::sine_blend::SineBlend::<DIAMOND_CENTER_LED_COUNT>::new(
                        Argb::DIAMOND_CENTER_WIFI_CONNECTING,
                        Argb::OFF,
                        4.0,
                        0.0,
                    )
                    .fade_in(1.5),
                );
                let master_volume = self.sound.volume();
                self.sound.set_master_volume(40);
                self.sound.queue(
                    sound::Type::Voice(sound::Voice::ShowWifiHotspotQrCode),
                    Duration::ZERO,
                )?;
                self.sound.set_master_volume(master_volume);
            }
            Event::NetworkConnectionSuccess => {
                self.stop_center(LEVEL_BACKGROUND, Transition::ForceStop);
                self.set_center(
                    LEVEL_BACKGROUND,
                    animations::Static::new(Argb::OFF, None),
                );
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
                self.operator_idle.api_mode(*api_mode);

                // transition to full ring when booting, from mcu to orb-ui
                if matches!(self.state, UiState::Booting) {
                    self.set_ring(
                        LEVEL_NOTICE,
                        animations::Alert::<DIAMOND_RING_LED_COUNT>::new(
                            Argb::DIAMOND_RING_BOOT_COMPLETE_IDLE,
                            BlinkDurations::from(vec![0.0, 2.0]),
                            Some(vec![0.5]),
                            false,
                        )?,
                    );
                }
                self.state =
                    UiState::Booted(if *api_mode { UiMode::Api } else { UiMode::Core });

                self.stop_center(LEVEL_FOREGROUND, Transition::ForceStop);
                self.stop_center(LEVEL_NOTICE, Transition::ForceStop);
                self.stop_ring(LEVEL_FOREGROUND, Transition::ForceStop);
                // make sure we set the background to off and stop all animations.
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
                self.capture_succeeded = false;
                self.capture_sound.reset();
                self.sound.queue(
                    sound::Type::Melody(sound::Melody::StartSignup),
                    Duration::ZERO,
                )?;

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
                    QrScanSchema::OperatorSelfServe | QrScanSchema::Operator => {
                        self.stop_center(LEVEL_NOTICE, Transition::ForceStop);
                        self.stop_center(LEVEL_BACKGROUND, Transition::ForceStop);
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
                                Argb::DIAMOND_CENTER_OPERATOR_QR_SCAN,
                                None,
                            )
                            .fade_in(1.5),
                        );
                    }
                    QrScanSchema::Wifi => {
                        if matches!(self.state, UiState::Booting) {
                            // default to orb-core mode at that stage
                            // the BootComplete event might overwrite the state
                            self.state = UiState::Booted(UiMode::Core);
                        }
                        self.operator_idle.no_wlan();
                        self.stop_ring(LEVEL_FOREGROUND, Transition::ForceStop);
                        self.stop_ring(LEVEL_NOTICE, Transition::ForceStop);
                        self.stop_ring(LEVEL_BACKGROUND, Transition::ForceStop);
                        self.set_center(
                            LEVEL_BACKGROUND,
                            animations::sine_blend::SineBlend::new(
                                Argb::DIAMOND_CENTER_WIFI_QR_SCAN,
                                Argb::OFF,
                                4.0,
                                0.0,
                            )
                            .fade_in(1.5),
                        );
                        // temporarily increase the volume to ask wifi qr code
                        let master_volume = self.sound.volume();
                        self.sound.set_master_volume(40);
                        self.sound.queue(
                            sound::Type::Voice(sound::Voice::ShowWifiHotspotQrCode),
                            Duration::ZERO,
                        )?;
                        self.sound.set_master_volume(master_volume);
                    }
                    QrScanSchema::User => {
                        self.stop_ring(LEVEL_FOREGROUND, Transition::ForceStop);
                        self.stop_ring(LEVEL_NOTICE, Transition::ForceStop);
                        self.stop_ring(LEVEL_BACKGROUND, Transition::ForceStop);
                        self.set_ring(
                            LEVEL_BACKGROUND,
                            animations::Static::<DIAMOND_RING_LED_COUNT>::new(
                                Argb::OFF,
                                None,
                            ),
                        );
                        self.set_center(
                            LEVEL_BACKGROUND,
                            animations::Static::<DIAMOND_CENTER_LED_COUNT>::new(
                                Argb::DIAMOND_CENTER_USER_QR_SCAN,
                                None,
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
            Event::QrScanCompleted { schema } => {
                match schema {
                    QrScanSchema::Operator => {}
                    QrScanSchema::OperatorSelfServe => {}
                    QrScanSchema::User => {
                        self.stop_ring(LEVEL_FOREGROUND, Transition::ForceStop);
                        self.stop_ring(LEVEL_NOTICE, Transition::ForceStop);
                        self.stop_ring(LEVEL_BACKGROUND, Transition::ForceStop);
                        self.set_center(
                            LEVEL_FOREGROUND,
                            animations::sine_blend::SineBlend::<DIAMOND_CENTER_LED_COUNT>::new(
                                Argb::DIAMOND_CENTER_USER_QR_SCAN_SUCCESS_COOL_WHITE,
                                Argb::OFF,
                                2.0,
                                0.0,
                            ),
                        );
                        self.sound.queue(
                            sound::Type::Melody(sound::Melody::QrLoadSuccess),
                            Duration::ZERO,
                        )?;
                    }
                    QrScanSchema::Wifi => {}
                };
            }
            Event::QrScanUnexpected { schema: _, reason } => {
                self.set_ring(
                    LEVEL_NOTICE,
                    animations::Alert::<DIAMOND_RING_LED_COUNT>::new(
                        Argb::DIAMOND_RING_ERROR_SALMON,
                        BlinkDurations::from(vec![0.0, 1.5, 4.0]),
                        Some(vec![0.5, 1.5]),
                        true,
                    )?,
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
                        self.set_ring(
                            LEVEL_NOTICE,
                            animations::Alert::<DIAMOND_RING_LED_COUNT>::new(
                                Argb::DIAMOND_RING_ERROR_SALMON,
                                BlinkDurations::from(vec![0.0, 1.5, 4.0]),
                                Some(vec![0.5, 1.5]),
                                true,
                            )?,
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
                    self.stop_ring(LEVEL_FOREGROUND, Transition::ForceStop);
                }
                QrScanSchema::User => {}
                QrScanSchema::Wifi => {
                    self.set_center(
                        LEVEL_FOREGROUND,
                        animations::Static::<DIAMOND_CENTER_LED_COUNT>::new(
                            Argb::DIAMOND_CENTER_WIFI_CONNECTING,
                            None,
                        )
                        .fade_in(0.5),
                    );
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
                        // show error animation
                        self.stop_ring(LEVEL_FOREGROUND, Transition::FadeOut(1.0));
                        self.set_center(
                            LEVEL_NOTICE,
                            animations::Alert::<DIAMOND_CENTER_LED_COUNT>::new(
                                Argb::DIAMOND_RING_ERROR_SALMON,
                                BlinkDurations::from(vec![0.0, 1.5, 4.0]),
                                Some(vec![0.5, 1.5]),
                                true,
                            )?,
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
            }
            Event::SignupStart => {
                self.capture_succeeded = false;
                self.capture_sound.reset();
                // if not self-serve, the animations to transition
                // to biometric capture are already set in `QrScanSuccess`
                self.sound.queue(
                    sound::Type::Melody(sound::Melody::UserStartCapture),
                    Duration::ZERO,
                )?;
            }
            Event::BiometricCaptureHalfObjectivesCompleted => {
                // do nothing
            }
            Event::BiometricCaptureAllObjectivesCompleted => {
                // do nothing
            }
            Event::BiometricCaptureProgress { progress } => {
                if self.capture_succeeded {
                    return Ok(());
                }
                // set progress but wait for wave to finish breathing
                let breathing = self
                    .ring_animations_stack
                    .stack
                    .get_mut(&LEVEL_FOREGROUND)
                    .and_then(|RunningAnimation { animation, .. }| {
                        animation
                            .as_any_mut()
                            .downcast_mut::<animations::Wave<DIAMOND_RING_LED_COUNT>>()
                    })
                    .is_some();
                if !breathing {
                    if self
                        .ring_animations_stack
                        .stack
                        .get_mut(&LEVEL_NOTICE)
                        .and_then(|RunningAnimation { animation, .. }| {
                            animation.as_any_mut().downcast_mut::<animations::Progress<
                                DIAMOND_RING_LED_COUNT,
                            >>()
                        })
                        .is_none()
                        || *progress <= 0.01
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
                            animation.as_any_mut().downcast_mut::<animations::Progress<
                                DIAMOND_RING_LED_COUNT,
                            >>()
                        });
                    if let Some(ring_progress) = ring_progress {
                        ring_progress.set_progress(*progress, None);
                    }
                }
            }
            Event::BiometricCaptureFakeProgressStart {
                timeout,
                min_fast_forward_duration,
                max_fast_forward_duration,
            } => {
                self.set_center(
                    LEVEL_FOREGROUND,
                    animations::Static::<DIAMOND_CENTER_LED_COUNT>::new(
                        Argb::DIAMOND_CENTER_BIOMETRIC_CAPTURE_PROGRESS,
                        None,
                    ),
                );
                self.set_ring(
                    LEVEL_NOTICE,
                    animations::fake_progress_v2::FakeProgress::<DIAMOND_RING_LED_COUNT>::new(
                        fake_progress_ring_color(),
                        *timeout,
                        *min_fast_forward_duration,
                        *max_fast_forward_duration,
                    ),
                );
            }
            Event::BiometricCaputreSuccessAndFastForwardFakeProgress => {
                let (ring_completion_time, ring_color) = self
                    .ring_animations_stack
                    .stack
                    .get_mut(&LEVEL_NOTICE)
                    .and_then(|RunningAnimation { animation, .. }| {
                        animation
                            .as_any_mut()
                            .downcast_mut::<animations::fake_progress_v2::FakeProgress<
                                DIAMOND_RING_LED_COUNT,
                            >>()
                    })
                    .map(|fake_progress| {
                        let color = fake_progress.get_color();
                        let completion_time = fake_progress.set_completed();
                        (completion_time, color)
                    })
                    .unwrap_or_default();
                let ring_completion_time = ring_completion_time.as_secs_f64();

                let mut total_duration = 0.0;
                while let Some(melody) = self.capture_sound.peekable().peek() {
                    let melody = sound::Type::Melody(*melody);
                    let melody_duration =
                        self.sound.get_duration(melody).unwrap().as_secs_f64();
                    if total_duration + melody_duration < ring_completion_time {
                        self.sound.queue(melody, Duration::ZERO)?;
                        self.capture_sound.next();
                        total_duration += melody_duration;
                    } else {
                        break;
                    }
                }

                // Sync biometric capture success animation + sounds, with the fake progress.
                // Since the ring is ON after the fake progress, we turn it off smoothly in `fade_out_duration`,
                // and then we do a double blink after `success_delay`.
                let fade_out_duration = 0.3;
                let success_delay = 0.4;
                self.sound.queue(
                    sound::Type::Melody(sound::Melody::IrisScanSuccess),
                    Duration::from_millis(
                        ((ring_completion_time + fade_out_duration + success_delay)
                            * 1000.0) as u64,
                    ),
                )?;
                self.set_center(
                    LEVEL_FOREGROUND,
                    animations::alert_v2::Alert::<DIAMOND_CENTER_LED_COUNT>::new(
                        Argb::DIAMOND_CENTER_BIOMETRIC_CAPTURE_PROGRESS,
                        SquarePulseTrain::from(vec![
                            (0.0, 0.0),
                            (fade_out_duration + success_delay + 1.1, 3.4),
                        ]),
                    )?
                    .with_delay(ring_completion_time),
                );
                self.set_ring(
                    LEVEL_FOREGROUND,
                    animations::alert_v2::Alert::<DIAMOND_RING_LED_COUNT>::new(
                        ring_color,
                        SquarePulseTrain::from(vec![
                            (0.0, 0.0),
                            (0.0, fade_out_duration),
                            (fade_out_duration + success_delay, 0.1),
                            (fade_out_duration + success_delay + 0.5, 0.1),
                            (fade_out_duration + success_delay + 1.0, 0.1),
                            (fade_out_duration + success_delay + 1.1, 3.5),
                        ]),
                    )?
                    .with_delay(ring_completion_time),
                );
            }
            Event::BiometricCaptureProgressWithNotch { progress } => {
                // set progress but wait for wave to finish breathing
                let breathing = self
                    .ring_animations_stack
                    .stack
                    .get_mut(&LEVEL_FOREGROUND)
                    .and_then(|RunningAnimation { animation, .. }| {
                        animation
                            .as_any_mut()
                            .downcast_mut::<animations::Wave<DIAMOND_RING_LED_COUNT>>()
                    })
                    .is_some();
                if !breathing {
                    if self
                        .ring_animations_stack
                        .stack
                        .get_mut(&LEVEL_NOTICE)
                        .and_then(|RunningAnimation { animation, .. }| {
                            animation.as_any_mut().downcast_mut::<animations::ProgressWithNotch<
                                DIAMOND_RING_LED_COUNT,
                            >>()
                        })
                        .is_none()
                        || *progress <= 0.01
                    {
                        // in case animation not yet initialized, initialize
                        self.set_ring(
                            LEVEL_NOTICE,
                            animations::ProgressWithNotch::<DIAMOND_RING_LED_COUNT>::new(
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
                                    .downcast_mut::<animations::ProgressWithNotch<
                                        DIAMOND_RING_LED_COUNT,
                                    >>()
                        });
                    if let Some(ring_progress) = ring_progress {
                        ring_progress.set_progress(*progress, None);
                    }
                }
            }
            Event::BiometricCaptureOcclusion { occlusion_detected } => {
                if *occlusion_detected {
                    // always freeze the progress bar on every occlusion event
                    if let Some(fake_progress) = self
                        .ring_animations_stack
                        .stack
                        .get_mut(&LEVEL_NOTICE)
                        .and_then(|RunningAnimation { animation, .. }| {
                            animation
                                .as_any_mut()
                                .downcast_mut::<animations::fake_progress_v2::FakeProgress<
                                    DIAMOND_RING_LED_COUNT,
                                >>()
                        })
                    {
                        fake_progress.halt();
                    } else if let Some(biometric_flow) = self
                        .ring_animations_stack
                        .stack
                        .get_mut(&LEVEL_NOTICE)
                        .and_then(|RunningAnimation { animation, .. }| {
                            animation
                                .as_any_mut()
                                .downcast_mut::<animations::composites::biometric_flow::BiometricFlow<
                                    DIAMOND_RING_LED_COUNT,
                                >>()
                        })
                    {
                        biometric_flow.halt_progress();
                    }
                    // animation + sound only on the first transition into occlusion
                    if !self.occlusion_active {
                        self.occlusion_active = true;
                        self.set_occlusion_center_animation();
                        self.occlusion_sound_last_played =
                            Some(std::time::Instant::now());
                        self.sound.queue(
                            sound::Type::Melody(sound::Melody::SoundError),
                            Duration::ZERO,
                        )?;
                    } else if self.occlusion_sound_last_played.map_or(false, |t| {
                        t.elapsed() >= std::time::Duration::from_secs(20)
                    }) {
                        self.occlusion_sound_last_played =
                            Some(std::time::Instant::now());
                        self.sound.queue(
                            sound::Type::Melody(sound::Melody::SoundError),
                            Duration::ZERO,
                        )?;
                    }
                } else {
                    self.occlusion_active = false;
                    self.occlusion_sound_last_played = None;
                    // Restore center LED to the appropriate distance-based state.
                    if self.capture_distance_in_range {
                        self.set_center(
                            LEVEL_NOTICE,
                            animations::Static::<DIAMOND_CENTER_LED_COUNT>::new(
                                Argb::DIAMOND_CENTER_BIOMETRIC_CAPTURE_PROGRESS,
                                None,
                            ),
                        );
                    } else {
                        self.set_occlusion_center_animation();
                    }
                    if self.capture_distance_in_range {
                        if let Some(fake_progress) = self
                            .ring_animations_stack
                            .stack
                            .get_mut(&LEVEL_NOTICE)
                            .and_then(|RunningAnimation { animation, .. }| {
                                animation
                                    .as_any_mut()
                                    .downcast_mut::<animations::fake_progress_v2::FakeProgress<
                                        DIAMOND_RING_LED_COUNT,
                                    >>()
                            })
                        {
                            fake_progress.resume();
                        } else if let Some(biometric_flow) = self
                            .ring_animations_stack
                            .stack
                            .get_mut(&LEVEL_NOTICE)
                            .and_then(|RunningAnimation { animation, .. }| {
                                animation
                                    .as_any_mut()
                                    .downcast_mut::<animations::composites::biometric_flow::BiometricFlow<
                                        DIAMOND_RING_LED_COUNT,
                                    >>()
                            })
                        {
                            biometric_flow.resume_progress();
                        }
                    }
                }
            }
            Event::BiometricCaptureDistance { in_range } => {
                // Don't override the occlusion breathing animation when occlusion is active.
                if !self.occlusion_active {
                    if *in_range {
                        self.set_center(
                            LEVEL_NOTICE,
                            animations::Static::<DIAMOND_CENTER_LED_COUNT>::new(
                                Argb::DIAMOND_CENTER_BIOMETRIC_CAPTURE_PROGRESS,
                                None,
                            ),
                        );
                    } else {
                        self.set_occlusion_center_animation();
                    }
                }
                // halt or resume both FakeProgress and BiometricFlow.
                // Only resume if both in range AND no occlusion active.
                self.capture_distance_in_range = *in_range;
                if let Some(fake_progress) = self
                    .ring_animations_stack
                    .stack
                    .get_mut(&LEVEL_NOTICE)
                    .and_then(|RunningAnimation { animation, .. }| {
                        animation
                            .as_any_mut()
                            .downcast_mut::<animations::fake_progress_v2::FakeProgress<
                                DIAMOND_RING_LED_COUNT,
                            >>()
                    })
                {
                    if *in_range && !self.occlusion_active {
                        fake_progress.resume();
                    } else if !in_range {
                        fake_progress.halt();
                    }
                } else if let Some(biometric_flow) = self
                    .ring_animations_stack
                    .stack
                    .get_mut(&LEVEL_NOTICE)
                    .and_then(|RunningAnimation { animation, .. }| {
                        animation
                            .as_any_mut()
                            .downcast_mut::<animations::composites::biometric_flow::BiometricFlow<
                                DIAMOND_RING_LED_COUNT,
                            >>()
                    })
                {
                    if *in_range {
                        biometric_flow.resume_progress();
                        if let Some(melody) = self.capture_sound.peekable().peek()
                            && self.sound.try_queue(sound::Type::Melody(*melody))?
                        {
                            self.capture_sound.next();
                        }
                    } else {
                        biometric_flow.halt_progress();
                        self.capture_sound = sound::capture::CaptureLoopSound::default();
                        let _ = self
                            .sound
                            .try_queue(sound::Type::Voice(sound::Voice::Silence));
                    }
                }
            }
            Event::BiometricFlowStart {
                timeout,
                min_fast_forward_duration,
                max_fast_forward_duration,
            } => {
                self.set_center(
                    LEVEL_FOREGROUND,
                    animations::Static::<DIAMOND_CENTER_LED_COUNT>::new(
                        Argb::DIAMOND_CENTER_BIOMETRIC_CAPTURE_PROGRESS,
                        None,
                    ),
                );
                self.set_ring(
                    LEVEL_NOTICE,
                    animations::composites::biometric_flow::BiometricFlow::<
                        DIAMOND_RING_LED_COUNT,
                    >::new(
                        fake_progress_ring_color(),
                        *timeout,
                        *min_fast_forward_duration,
                        *max_fast_forward_duration,
                        fake_progress_ring_color(),
                        Argb::DIAMOND_RING_ERROR_SALMON,
                    ),
                );
            }
            Event::BiometricFlowProgressFastForward => {
                if let Some(biometric_flow) = self
                .ring_animations_stack
                .stack
                .get_mut(&LEVEL_NOTICE)
                .and_then(|RunningAnimation { animation, .. }| {
                    animation.as_any_mut().downcast_mut::<animations::composites::biometric_flow::BiometricFlow<DIAMOND_RING_LED_COUNT>>()
                }) {
                    biometric_flow.progress_fast_forward();
                    let ring_completion_time = biometric_flow.get_progress_completion_time().as_secs_f64();

                    // Play biometric capture sound while the progress is running.
                    let mut total_duration = 0.0;
                    while let Some(melody) = self.capture_sound.peekable().peek() {
                        let melody = sound::Type::Melody(*melody);
                        let melody_duration =
                            self.sound.get_duration(melody).unwrap().as_secs_f64();
                        if total_duration + melody_duration < ring_completion_time {
                            self.sound.queue(melody, Duration::ZERO)?;
                            self.capture_sound.next();
                            total_duration += melody_duration;
                        } else {
                            break;
                        }
                    }
                }
            }
            Event::BiometricFlowResult { .. } => {}
            Event::BiometricCaptureSuccess | Event::BiometricCaptureSuccessGreen => {
                self.biometric_capture_success()?;
            }
            Event::BiometricPipelineProgress { progress: _ } => {
                // do nothing, for now
            }
            Event::StartingEnrollment => {
                // do nothing, for now
            }
            Event::BiometricPipelineSuccess => {
                // do nothing, for now
            }
            Event::SoundVolume { level } => {
                self.sound.set_master_volume(*level);
            }
            Event::SoundLanguage { lang } => {
                let language = lang.clone();
                let sound = self.sound.clone();
                // spawn a new task because we need some async work here
                tokio::task::spawn(async move {
                    match sound::SoundConfig::default()
                        .with_language(language.as_deref())
                    {
                        Ok(config) => {
                            if let Err(e) = sound.load_sound_files(config).await {
                                tracing::error!("Error loading sound files: {:?}", e);
                            }
                        }
                        Err(e) => {
                            tracing::error!("Error creating sound config: {:?}", e);
                        }
                    }
                });
            }
            Event::SoundTest => {
                self.sound.queue(
                    sound::Type::Melody(sound::Melody::BootUp),
                    Duration::ZERO,
                )?;
            }
            Event::SignupFail { reason } => match reason {
                SignupFailReason::Timeout => {
                    self.play_signup_fail_ux(Some(sound::Type::Voice(
                        sound::Voice::Timeout,
                    )))?;
                }
                SignupFailReason::FaceNotFound => {
                    self.play_signup_fail_ux(Some(sound::Type::Voice(
                        sound::Voice::FaceNotFound,
                    )))?;
                }
                SignupFailReason::Server => {}
                SignupFailReason::UploadCustodyImages => {}
                SignupFailReason::Verification => {}
                SignupFailReason::SoftwareVersionDeprecated => {}
                SignupFailReason::SoftwareVersionBlocked => {}
                SignupFailReason::Duplicate => {}
                SignupFailReason::Unknown => {}
                SignupFailReason::Aborted => {
                    self.play_signup_fail_ux(None)?;
                }
            },
            Event::SignupSuccess => {
                if !self.capture_succeeded {
                    self.biometric_capture_success()?;
                }
                self.stop_center(LEVEL_FOREGROUND, Transition::ForceStop);
                self.stop_center(LEVEL_NOTICE, Transition::ForceStop);
                self.set_center(
                    LEVEL_FOREGROUND,
                    animations::sine_blend::SineBlend::<DIAMOND_CENTER_LED_COUNT>::new(
                        Argb::DIAMOND_CENTER_BIOMETRIC_CAPTURE_SUCCESS_GREEN,
                        Argb::OFF,
                        4.0,
                        0.0,
                    ),
                );
            }
            Event::Idle => {
                self.capture_succeeded = false;
                self.stop_ring(LEVEL_FOREGROUND, Transition::ForceStop);
                self.stop_center(LEVEL_FOREGROUND, Transition::ForceStop);
                self.stop_ring(LEVEL_NOTICE, Transition::ForceStop);
                self.stop_center(LEVEL_NOTICE, Transition::ForceStop);

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
                        Argb::DIAMOND_RING_BOOT_COMPLETE_IDLE,
                        None,
                    )
                    .fade_in(1.5),
                );
            }

            Event::CriticalState {
                state: CriticalState::WifiModuleNotInitialized,
            } => {
                self.operator_idle.wlan_init_failure();
            }

            Event::VoiceOpenEyes => {
                self.sound.queue(
                    sound::Type::Voice(sound::Voice::OpenEyes),
                    Duration::ZERO,
                )?;
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

            Event::Gimbal { x, y } => {
                self.gimbal = Some((*x, *y));
            }
            _ => {}
        }
        Ok(())
    }

    async fn run(&mut self, interface_tx: &mut Sender<Message>) -> Result<()> {
        let dt = self.timer.get_dt().unwrap_or(0.0);
        self.center_animations_stack.run(&mut self.center_frame, dt);

        let paused = matches!(self.state, UiState::Paused(_));

        if !paused {
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
        if !paused {
            // 2ms sleep to make sure UART communication is over
            time::sleep(Duration::from_millis(2)).await;
            interface_tx
                .try_send(WrappedOperatorMessage::from(self.operator_frame).0)?;
        }

        self.ring_animations_stack.run(&mut self.ring_frame, dt);
        if !paused {
            time::sleep(Duration::from_millis(2)).await;
            interface_tx.try_send(WrappedRingMessage::from(self.ring_frame).0)?;
        }
        if let Some(animation) = &mut self.cone_animations_stack
            && let Some(frame) = &mut self.cone_frame
        {
            animation.run(frame, dt);
            if !paused {
                time::sleep(Duration::from_millis(2)).await;
                interface_tx.try_send(WrappedConeMessage::from(*frame).0)?;
            }
        }

        if let Some((x, y)) = self.gimbal {
            interface_tx.try_send(Message::JMessage(JetsonToMcu {
                ack_number: 0,
                payload: Some(jetson_to_mcu::Payload::MirrorAngle(
                    orb_messages::main::MirrorAngle {
                        horizontal_angle: 0,
                        vertical_angle: 0,
                        phi_angle_millidegrees: x,
                        theta_angle_millidegrees: y,
                        angle_type: orb_messages::main::MirrorAngleType::PhiTheta
                            as i32,
                    },
                )),
            }))?;

            // send only once
            self.gimbal = None;
        }

        if let UiState::Booted(_) = self.state {
            // send message to mcu to indicate orb is ready
            interface_tx.try_send(Message::JMessage(JetsonToMcu {
                ack_number: 0,
                payload: Some(jetson_to_mcu::Payload::BootComplete(
                    orb_messages::main::BootComplete {},
                )),
            }))?;
        }

        // one last update of the UI has been performed since api_mode has been set,
        // (to set the api_mode UI state), so we can now pause the engine
        match self.state {
            UiState::Booted(UiMode::Api) => {
                self.state = UiState::Paused(UiMode::Api);
                tracing::info!("UI paused in api mode");
            }
            UiState::Booted(UiMode::Core) => {
                self.state = UiState::Running(UiMode::Core);
                tracing::info!("UI running in core mode");
            }
            UiState::Booting | UiState::Running(_) | UiState::Paused(_) => {}
        }

        Ok(())
    }
}
