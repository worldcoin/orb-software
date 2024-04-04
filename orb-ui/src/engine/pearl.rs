use async_trait::async_trait;
use eyre::Result;
use futures::channel::mpsc;
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
    center, operator, ring, Animation, AnimationsStack, CenterFrame, Event,
    EventHandler, OperatorFrame, OrbType, QrScanSchema, RingFrame, Runner,
    RunningAnimation, SignupFailReason, BIOMETRIC_PIPELINE_MAX_PROGRESS,
    LED_ENGINE_FPS, LEVEL_BACKGROUND, LEVEL_FOREGROUND, LEVEL_NOTICE,
    PEARL_CENTER_LED_COUNT, PEARL_RING_LED_COUNT,
};
use crate::sound;
use crate::sound::Player;

struct WrappedMessage(Message);

impl From<CenterFrame<PEARL_CENTER_LED_COUNT>> for WrappedMessage {
    fn from(value: CenterFrame<PEARL_CENTER_LED_COUNT>) -> Self {
        WrappedMessage(Message::JMessage(
            JetsonToMcu {
                ack_number: 0,
                payload: Some(jetson_to_mcu::Payload::CenterLedsSequence(
                    orb_messages::mcu_main::UserCenterLeDsSequence {
                        data_format: Some(
                            orb_messages::mcu_main::user_center_le_ds_sequence::DataFormat::RgbUncompressed(
                                value.iter().flat_map(|&Argb(_, r, g, b)| [r, g, b]).collect(),
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
                    orb_messages::mcu_main::UserRingLeDsSequence {
                        data_format: Some(
                            orb_messages::mcu_main::user_ring_le_ds_sequence::DataFormat::RgbUncompressed(
                                value.iter().flat_map(|&Argb(_, r, g, b)| [r, g, b]).collect(),
                            ))
                    }
                )),
            }
        ))
    }
}

/// Dummy implementation, not used since Pearl cannot be connected to a cone
impl From<RingFrame<64>> for WrappedMessage {
    fn from(value: RingFrame<64>) -> Self {
        WrappedMessage(Message::JMessage(
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

impl From<OperatorFrame> for WrappedMessage {
    fn from(value: OperatorFrame) -> Self {
        WrappedMessage(Message::JMessage(
            JetsonToMcu {
                ack_number: 0,
                payload: Some(jetson_to_mcu::Payload::DistributorLedsSequence(
                    orb_messages::mcu_main::DistributorLeDsSequence {
                        data_format: Some(
                            orb_messages::mcu_main::distributor_le_ds_sequence::DataFormat::RgbUncompressed(
                                value.iter().flat_map(|&Argb(_, r, g, b)| [r, g, b]).collect(),
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
    let mut runner = if let Ok(sound) = sound::Jetson::spawn().await {
        Runner::<PEARL_RING_LED_COUNT, PEARL_CENTER_LED_COUNT>::new(sound)
    } else {
        return Err(eyre::eyre!("Failed to initialize sound"));
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

impl Runner<PEARL_RING_LED_COUNT, PEARL_CENTER_LED_COUNT> {
    pub(crate) fn new(sound: sound::Jetson) -> Self {
        Self {
            timer: InstantTimer::default(),
            ring_animations_stack: AnimationsStack::new(),
            center_animations_stack: AnimationsStack::new(),
            cone_animations_stack: None,
            ring_frame: [Argb(None, 0, 0, 0); PEARL_RING_LED_COUNT],
            center_frame: [Argb(None, 0, 0, 0); PEARL_CENTER_LED_COUNT],
            cone_frame: None,
            operator_frame: OperatorFrame::default(),
            operator_connection: operator::Connection::new(OrbType::Pearl),
            operator_battery: operator::Battery::new(OrbType::Pearl),
            operator_blink: operator::Blink::new(OrbType::Pearl),
            operator_pulse: operator::Pulse::new(OrbType::Pearl),
            operator_action: operator::Bar::new(OrbType::Pearl),
            operator_signup_phase: operator::SignupPhase::new(OrbType::Pearl),
            sound,
            capture_sound: sound::capture::CaptureLoopSound::default(),
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
    fn event(&mut self, event: &Event) -> Result<()> {
        tracing::info!("UI event: {:?}", event);
        match event {
            Event::Bootup => {
                self.sound
                    .queue(sound::Type::Melody(sound::Melody::BootUp))?;
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
                self.sound
                    .queue(sound::Type::Melody(sound::Melody::PoweringDown))?;
                // overwrite any existing animation by setting notice-level animation
                // as the last animation before shutdown
                self.set_center(
                    LEVEL_NOTICE,
                    center::Alert::<PEARL_CENTER_LED_COUNT>::new(
                        if *requested {
                            Argb::PEARL_USER_QR_SCAN
                        } else {
                            Argb::PEARL_USER_AMBER
                        },
                        vec![0.0, 0.3, 0.45, 0.3, 0.45, 0.45],
                        false,
                    ),
                );
                self.set_ring(
                    LEVEL_NOTICE,
                    ring::r#static::Static::<PEARL_RING_LED_COUNT>::new(
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
                    Argb::PEARL_OPERATOR_DEFAULT,
                    false,
                    true,
                    false,
                );
                self.operator_signup_phase.signup_phase_started();

                // stop all
                self.set_center(
                    LEVEL_BACKGROUND,
                    center::Static::<PEARL_CENTER_LED_COUNT>::new(Argb::OFF, None),
                );
                self.stop_ring(LEVEL_FOREGROUND, true);
                self.stop_center(LEVEL_FOREGROUND, true);
                self.stop_center(LEVEL_NOTICE, true);

                // reset ring background to black/off so that it's turned off in next animations
                self.set_ring(
                    LEVEL_BACKGROUND,
                    ring::r#static::Static::<PEARL_RING_LED_COUNT>::new(
                        Argb::OFF,
                        None,
                    ),
                );
            }
            Event::QrScanStart { schema } => {
                self.set_center(
                    LEVEL_FOREGROUND,
                    center::Wave::<PEARL_CENTER_LED_COUNT>::new(
                        Argb::PEARL_USER_QR_SCAN,
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
                        self.sound.queue(sound::Type::Voice(
                            sound::Voice::ShowWifiHotspotQrCode,
                        ))?;
                    }
                    QrScanSchema::User => {
                        self.operator_signup_phase.user_qr_code_ok();
                        // initialize ring with short segment to invite user to scan QR
                        self.set_ring(
                            LEVEL_FOREGROUND,
                            ring::Slider::<PEARL_RING_LED_COUNT>::new(
                                0.0,
                                Argb::PEARL_USER_SIGNUP,
                            ),
                        );
                    }
                };
            }
            Event::QrScanCapture => {
                // stop wave (foreground) & show alert/blinks (notice)
                self.stop_center(LEVEL_FOREGROUND, true);
                self.sound
                    .queue(sound::Type::Melody(sound::Melody::QrCodeCapture))?;
            }
            Event::QrScanCompleted { schema } => {
                // stop wave (foreground) & show alert/blinks (notice)
                self.stop_center(LEVEL_FOREGROUND, true);
                self.set_center(
                    LEVEL_NOTICE,
                    center::Alert::<PEARL_CENTER_LED_COUNT>::new(
                        Argb::PEARL_USER_QR_SCAN,
                        vec![0.0, 0.3, 0.45, 0.46],
                        false,
                    ),
                );

                match schema {
                    QrScanSchema::Operator => {
                        self.sound
                            .queue(sound::Type::Melody(sound::Melody::QrLoadSuccess))?;
                    }
                    QrScanSchema::User => {}
                    QrScanSchema::Wifi => {}
                }
            }
            Event::QrScanUnexpected { schema } => {
                self.sound
                    .queue(sound::Type::Voice(sound::Voice::QrCodeInvalid))?;
                match schema {
                    QrScanSchema::User => {
                        // remove short segment from ring
                        self.stop_ring(LEVEL_FOREGROUND, true);
                        self.operator_signup_phase.user_qr_code_issue();
                    }
                    QrScanSchema::Operator => {
                        self.operator_signup_phase.operator_qr_code_issue();
                    }
                    QrScanSchema::Wifi => {}
                }
                // stop wave
                self.stop_center(LEVEL_FOREGROUND, true);
            }
            Event::QrScanFail { schema } => {
                self.sound
                    .queue(sound::Type::Melody(sound::Melody::SoundError))?;
                match schema {
                    QrScanSchema::User | QrScanSchema::Operator => {
                        // in case schema is user qr
                        self.stop_ring(LEVEL_FOREGROUND, true);
                        self.operator_signup_phase.failure();
                    }
                    QrScanSchema::Wifi => {}
                }
                // stop wave
                self.stop_center(LEVEL_FOREGROUND, true);
            }
            Event::QrScanSuccess { schema } => {
                match schema {
                    QrScanSchema::Operator => {
                        self.operator_signup_phase.operator_qr_captured();
                    }
                    QrScanSchema::User => {
                        self.sound.queue(sound::Type::Melody(
                            sound::Melody::UserQrLoadSuccess,
                        ))?;
                        self.operator_signup_phase.user_qr_captured();
                        // initialize ring with animated short segment to invite user to start iris capture
                        self.set_ring(
                            LEVEL_NOTICE,
                            ring::Slider::<PEARL_RING_LED_COUNT>::new(
                                0.0,
                                Argb::PEARL_USER_SIGNUP,
                            )
                            .with_pulsing(),
                        );
                        // remove short segment from ring (foreground, superseded by notice level above)
                        self.stop_ring(LEVEL_FOREGROUND, false);
                        // off background for biometric-capture, which relies on LEVEL_NOTICE animations
                        self.stop_center(LEVEL_FOREGROUND, true);
                        self.set_center(
                            LEVEL_FOREGROUND,
                            center::Static::<PEARL_CENTER_LED_COUNT>::new(
                                Argb::OFF,
                                None,
                            ),
                        );
                    }
                    QrScanSchema::Wifi => {
                        self.sound
                            .queue(sound::Type::Melody(sound::Melody::QrLoadSuccess))?;
                    }
                }
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
                            Argb::PEARL_USER_SIGNUP,
                        )
                        .with_pulsing(),
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
                    if let Some(melody) = self.capture_sound.peekable().peek() {
                        if self.sound.try_queue(sound::Type::Melody(*melody))? {
                            self.capture_sound.next();
                        }
                    }
                } else {
                    self.operator_signup_phase.capture_distance_issue();
                    self.capture_sound.restart_current_loop();
                    let _ = self
                        .sound
                        .try_queue(sound::Type::Voice(sound::Voice::Silence));
                }
            }
            Event::BiometricCaptureSuccess => {
                self.sound
                    .queue(sound::Type::Melody(sound::Melody::IrisScanSuccess))?;
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
                        x.set_progress(2.0, false);
                    });
                self.stop_center(LEVEL_NOTICE, false);
                self.stop_ring(LEVEL_NOTICE, false);

                // preparing animation for biometric pipeline progress
                self.set_ring(
                    LEVEL_FOREGROUND,
                    ring::Progress::<PEARL_RING_LED_COUNT>::new(
                        0.0,
                        None,
                        Argb::PEARL_USER_SIGNUP,
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
                            .downcast_mut::<ring::Progress<PEARL_RING_LED_COUNT>>()
                    });
                if let Some(ring_animation) = ring_animation {
                    ring_animation.set_progress(
                        *progress * BIOMETRIC_PIPELINE_MAX_PROGRESS,
                        None,
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
                    SignupFailReason::Duplicate => {}
                    SignupFailReason::Unknown => {}
                }
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
                self.stop_center(LEVEL_FOREGROUND, true);
                self.set_ring(
                    LEVEL_BACKGROUND,
                    ring::Idle::<PEARL_RING_LED_COUNT>::new(
                        Some(Argb::PEARL_USER_SIGNUP),
                        Some(1.0),
                    ),
                );
            }
            Event::SignupSuccess => {
                self.sound
                    .queue(sound::Type::Melody(sound::Melody::SignupSuccess))?;

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
                self.stop_center(LEVEL_FOREGROUND, true);
                self.set_ring(
                    LEVEL_BACKGROUND,
                    ring::Idle::<PEARL_RING_LED_COUNT>::new(
                        Some(Argb::PEARL_USER_SIGNUP),
                        Some(3.0),
                    ),
                );
            }
            Event::SoftwareVersionDeprecated => {
                self.sound
                    .queue(sound::Type::Melody(sound::Melody::SoundError))?;

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
                self.stop_center(LEVEL_FOREGROUND, true);
                self.operator_blink.trigger(
                    Argb::PEARL_OPERATOR_VERSIONS_DEPRECATED,
                    vec![0.4, 0.4, 0.4, 0.4, 0.4, 0.4],
                );
            }
            Event::SoftwareVersionBlocked => {
                self.sound
                    .queue(sound::Type::Melody(sound::Melody::SoundError))?;

                let slider = self
                    .ring_animations_stack
                    .stack
                    .get_mut(&LEVEL_NOTICE)
                    .and_then(|RunningAnimation { animation, .. }| {
                        animation
                            .as_any_mut()
                            .downcast_mut::<ring::Progress<PEARL_RING_LED_COUNT>>()
                    });
                if let Some(slider) = slider {
                    slider.set_progress(2.0, None);
                }
                self.stop_ring(LEVEL_FOREGROUND, false);
                self.stop_center(LEVEL_FOREGROUND, true);
                self.operator_blink.trigger(
                    Argb::PEARL_OPERATOR_VERSIONS_OUTDATED,
                    vec![0.4, 0.4, 0.4, 0.4, 0.4, 0.4],
                );
            }
            Event::Idle => {
                self.stop_ring(LEVEL_FOREGROUND, false);
                self.stop_ring(LEVEL_NOTICE, false);
                self.stop_center(LEVEL_FOREGROUND, false);
                self.stop_center(LEVEL_NOTICE, false);
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
                            .downcast_mut::<ring::Spinner<PEARL_RING_LED_COUNT>>()
                    })
                    .is_none()
                {
                    self.set_ring(
                        LEVEL_NOTICE,
                        ring::Spinner::<PEARL_RING_LED_COUNT>::triple(
                            Argb::PEARL_USER_RED,
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
            Event::SoundLanguage { lang: _lang } => {
                // fixme
            }
        }
        Ok(())
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
        if !self.paused {
            // 2ms sleep to make sure UART communication is over
            time::sleep(Duration::from_millis(2)).await;
            interface_tx.try_send(WrappedMessage::from(self.operator_frame).0)?;
        }

        self.ring_animations_stack.run(&mut self.ring_frame, dt);
        if !self.paused {
            time::sleep(Duration::from_millis(2)).await;
            interface_tx.try_send(WrappedMessage::from(self.ring_frame).0)?;
        }
        if let Some(animation) = &mut self.cone_animations_stack {
            if let Some(frame) = &mut self.cone_frame {
                animation.run(frame, dt);
                if !self.paused {
                    time::sleep(Duration::from_millis(2)).await;
                    interface_tx.try_send(WrappedMessage::from(*frame).0)?;
                }
            }
        }
        Ok(())
    }
}
