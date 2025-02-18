use crate::engine::{
    animations, Animation, Event, QrScanSchema, QrScanUnexpectedReason, Runner,
    RunningAnimation, SignupFailReason, Transition, LEVEL_BACKGROUND, LEVEL_FOREGROUND,
    LEVEL_NOTICE, PEARL_CENTER_LED_COUNT, PEARL_RING_LED_COUNT,
};
use crate::sound;
use crate::sound::Player;
use animations::alert::BlinkDurations;
use eyre::Result;
use orb_rgb::Argb;
use std::f64::consts::PI;
use std::time::Duration;

impl Runner<PEARL_RING_LED_COUNT, PEARL_CENTER_LED_COUNT> {
    pub(super) fn event_self_serve(&mut self, event: &Event) -> Result<()> {
        match event {
            Event::Bootup => {
                self.stop_ring(LEVEL_NOTICE, Transition::ForceStop);
                self.stop_center(LEVEL_NOTICE, Transition::ForceStop);
                self.set_ring(
                    LEVEL_BACKGROUND,
                    animations::Idle::<PEARL_RING_LED_COUNT>::default(),
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
                    animations::Static::<PEARL_CENTER_LED_COUNT>::new(Argb::OFF, None),
                );
                self.set_ring(
                    LEVEL_BACKGROUND,
                    animations::Static::<PEARL_RING_LED_COUNT>::new(Argb::OFF, None),
                );
            }
            Event::Shutdown { requested: _ } => {
                self.sound.queue(
                    sound::Type::Melody(sound::Melody::PoweringDown),
                    Duration::ZERO,
                )?;
                self.set_ring(
                    LEVEL_NOTICE,
                    animations::Static::<PEARL_RING_LED_COUNT>::new(Argb::OFF, None),
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
                    Argb::PEARL_OPERATOR_DEFAULT,
                    false,
                    true,
                    false,
                );
                self.operator_signup_phase.signup_phase_started();

                // stop all
                self.set_center(
                    LEVEL_BACKGROUND,
                    animations::Static::<PEARL_CENTER_LED_COUNT>::new(Argb::OFF, None),
                );
                self.stop_center(LEVEL_FOREGROUND, Transition::ForceStop);
                self.stop_center(LEVEL_NOTICE, Transition::ForceStop);

                self.set_ring(
                    LEVEL_BACKGROUND,
                    animations::Static::<PEARL_RING_LED_COUNT>::new(Argb::OFF, None),
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
                                Argb::PEARL_RING_OPERATOR_QR_SCAN_SPINNER,
                                Some(Argb::PEARL_RING_OPERATOR_QR_SCAN),
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
                                Argb::PEARL_RING_OPERATOR_QR_SCAN_SPINNER_OPERATOR_BASED,
                                Some(Argb::OFF),
                            )
                                .fade_in(1.5),
                        );
                        self.operator_signup_phase.operator_qr_code_ok();
                    }
                    QrScanSchema::Wifi => {
                        self.operator_idle.no_wlan();
                        self.set_ring(
                            LEVEL_FOREGROUND,
                            animations::SimpleSpinner::new(
                                Argb::PEARL_RING_WIFI_QR_SCAN_SPINNER,
                                Some(Argb::PEARL_RING_WIFI_QR_SCAN),
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
                        self.operator_signup_phase.user_qr_code_ok();
                        self.set_ring(
                            LEVEL_FOREGROUND,
                            animations::SimpleSpinner::new(
                                Argb::PEARL_RING_USER_QR_SCAN_SPINNER,
                                Some(Argb::PEARL_RING_USER_QR_SCAN),
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
                    animations::Static::<PEARL_RING_LED_COUNT>::new(Argb::OFF, None),
                );

                // use previous background color to blink
                let bg_color = if let Some(spinner) = self
                    .ring_animations_stack
                    .stack
                    .get_mut(&LEVEL_FOREGROUND)
                    .and_then(|RunningAnimation { animation, .. }| {
                        animation
                            .as_any_mut()
                            .downcast_mut::<animations::SimpleSpinner<
                                PEARL_RING_LED_COUNT,
                            >>()
                    }) {
                    spinner.background()
                } else {
                    Argb::OFF
                };
                // 2-blink alert + fade-out
                self.set_ring(
                    LEVEL_NOTICE,
                    animations::Alert::<PEARL_RING_LED_COUNT>::new(
                        bg_color,
                        BlinkDurations::from(vec![0.0, 0.4, 0.2, 0.4]),
                        Some(vec![0.2, 0.2, 0.01]),
                        true,
                    ),
                );
            }
            Event::QrScanUnexpected { schema, reason } => {
                self.set_ring(
                    LEVEL_NOTICE,
                    animations::Alert::<PEARL_RING_LED_COUNT>::new(
                        Argb::PEARL_RING_ERROR_SALMON,
                        BlinkDurations::from(vec![0.0, 1.5, 4.0]),
                        Some(vec![0.5, 1.5]),
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
                            animations::Alert::<PEARL_RING_LED_COUNT>::new(
                                Argb::PEARL_RING_ERROR_SALMON,
                                BlinkDurations::from(vec![0.0, 1.5, 4.0]),
                                Some(vec![0.5, 1.5]),
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
                            Argb::PEARL_RING_USER_QR_SCAN_SPINNER,
                            Some(Argb::PEARL_RING_USER_QR_SCAN),
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
                        self.stop_ring(LEVEL_FOREGROUND, Transition::FadeOut(1.0));
                        self.set_center(
                            LEVEL_NOTICE,
                            animations::Alert::<PEARL_CENTER_LED_COUNT>::new(
                                Argb::PEARL_RING_ERROR_SALMON,
                                BlinkDurations::from(vec![0.0, 1.5, 4.0]),
                                Some(vec![0.5, 1.5]),
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
                // pulsing wave animation displayed
                // while we wait for the user to be in position
                self.set_ring(
                    LEVEL_FOREGROUND,
                    animations::Wave::<PEARL_RING_LED_COUNT>::new(
                        Argb::PEARL_CENTER_SUMMON_USER_AMBER,
                        3.0,
                        0.0,
                        false,
                    ),
                );
            }
            Event::BiometricCaptureHalfObjectivesCompleted => {
                // do nothing
            }
            Event::BiometricCaptureAllObjectivesCompleted => {
                self.operator_signup_phase.irises_captured();
            }
            Event::BiometricCaptureProgress { progress } => {
                // set progress but wait for wave to finish breathing
                let breathing = self
                    .ring_animations_stack
                    .stack
                    .get_mut(&LEVEL_FOREGROUND)
                    .and_then(|RunningAnimation { animation, .. }| {
                        animation
                            .as_any_mut()
                            .downcast_mut::<animations::Wave<PEARL_RING_LED_COUNT>>()
                    })
                    .is_some();
                if !breathing {
                    if self
                        .ring_animations_stack
                        .stack
                        .get_mut(&LEVEL_NOTICE)
                        .and_then(|RunningAnimation { animation, .. }| {
                            animation.as_any_mut().downcast_mut::<animations::Progress<
                                PEARL_RING_LED_COUNT,
                            >>()
                        })
                        .is_none()
                        || *progress <= 0.01
                    {
                        // in case animation not yet initialized, initialize
                        self.set_ring(
                            LEVEL_NOTICE,
                            animations::Progress::<PEARL_RING_LED_COUNT>::new(
                                0.0,
                                None,
                                Argb::PEARL_RING_USER_CAPTURE,
                            ),
                        );
                    }
                    let ring_progress = self
                        .ring_animations_stack
                        .stack
                        .get_mut(&LEVEL_NOTICE)
                        .and_then(|RunningAnimation { animation, .. }| {
                            animation.as_any_mut().downcast_mut::<animations::Progress<
                                PEARL_RING_LED_COUNT,
                            >>()
                        });
                    if let Some(ring_progress) = ring_progress {
                        ring_progress.set_progress(*progress, None);
                    }
                }
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
                            .downcast_mut::<animations::Wave<PEARL_RING_LED_COUNT>>()
                    })
                    .is_some();
                if !breathing {
                    if self
                        .ring_animations_stack
                        .stack
                        .get_mut(&LEVEL_NOTICE)
                        .and_then(|RunningAnimation { animation, .. }| {
                            animation.as_any_mut().downcast_mut::<animations::ProgressWithNotch<
                                PEARL_RING_LED_COUNT,
                            >>()
                        })
                        .is_none()
                        || *progress <= 0.01
                    {
                        // in case animation not yet initialized, initialize
                        self.set_ring(
                            LEVEL_NOTICE,
                            animations::ProgressWithNotch::<PEARL_RING_LED_COUNT>::new(
                                0.0,
                                None,
                                Argb::PEARL_RING_USER_CAPTURE,
                            ),
                        );
                    }
                    let ring_progress = self
                        .ring_animations_stack
                        .stack
                        .get_mut(&LEVEL_NOTICE)
                        .and_then(|RunningAnimation { animation, .. }| {
                            animation.as_any_mut().downcast_mut::<animations::ProgressWithNotch<
                                PEARL_RING_LED_COUNT,
                            >>()
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
                // only once we stop breathing
                let breathing = self
                    .ring_animations_stack
                    .stack
                    .get_mut(&LEVEL_FOREGROUND)
                    .and_then(|RunningAnimation { animation, .. }| {
                        animation
                            .as_any_mut()
                            .downcast_mut::<animations::Wave<PEARL_RING_LED_COUNT>>()
                    })
                    .is_some();
                if breathing && *in_range {
                    // stop any ongoing breathing animation
                    self.stop_ring(LEVEL_FOREGROUND, Transition::ForceStop);
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
                    animations::Static::<PEARL_CENTER_LED_COUNT>::new(Argb::OFF, None),
                );
                self.stop_ring(LEVEL_FOREGROUND, Transition::ForceStop);
                let success_alert_blinks =
                    vec![0.0, fade_out_duration, 0.5, 0.75, 0.5, 1.5, 0.5, 3.0, 0.2];
                let alert_duration = success_alert_blinks.iter().sum::<f64>();
                self.set_ring(
                    LEVEL_NOTICE,
                    animations::Alert::<PEARL_RING_LED_COUNT>::new(
                        Argb::FULL_GREEN,
                        BlinkDurations::from(success_alert_blinks),
                        Some(vec![0.1, 0.4, 0.4, 0.2, 0.75]),
                        false,
                    ),
                );
                self.set_ring(
                    LEVEL_FOREGROUND,
                    animations::Wave::<PEARL_RING_LED_COUNT>::new(
                        Argb::PEARL_RING_USER_CAPTURE,
                        3.0,
                        0.0,
                        true,
                    )
                    .with_delay(alert_duration),
                );
                self.operator_signup_phase.iris_scan_complete();
            }
            Event::BiometricPipelineProgress { progress } => {
                // operator LED to show pipeline progress
                if *progress <= 0.5 {
                    self.operator_signup_phase.processing_1();
                } else {
                    self.operator_signup_phase.processing_2();
                }
            }
            Event::StartingEnrollment => {
                self.operator_signup_phase.uploading();
            }
            Event::BiometricPipelineSuccess => {
                self.operator_signup_phase.biometric_pipeline_successful();
            }
            Event::SignupFail { reason } => {
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
                            Argb::PEARL_OPERATOR_VERSIONS_DEPRECATED,
                            vec![0.4, 0.4, 0.4, 0.4, 0.4, 0.4],
                        );
                    }
                    SignupFailReason::SoftwareVersionBlocked => {
                        self.operator_blink.trigger(
                            Argb::PEARL_OPERATOR_VERSIONS_OUTDATED,
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

                // ring, run error animation at NOTICE level, off for the rest.
                self.set_ring(
                    LEVEL_BACKGROUND,
                    animations::Static::<PEARL_RING_LED_COUNT>::new(Argb::OFF, None),
                );
                self.stop_ring(LEVEL_FOREGROUND, Transition::ForceStop);
                self.stop_ring(LEVEL_NOTICE, Transition::FadeOut(1.0));
                self.set_center(
                    LEVEL_NOTICE,
                    animations::Alert::<PEARL_CENTER_LED_COUNT>::new(
                        Argb::PEARL_RING_ERROR_SALMON,
                        BlinkDurations::from(vec![0.0, 1.5, 4.0]),
                        Some(vec![0.5, 1.5]),
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

                self.set_ring(
                    LEVEL_BACKGROUND,
                    animations::Static::<PEARL_RING_LED_COUNT>::new(Argb::OFF, None),
                );
                self.stop_ring(LEVEL_FOREGROUND, Transition::ForceStop);
                self.set_ring(
                    LEVEL_NOTICE,
                    animations::Alert::<PEARL_RING_LED_COUNT>::new(
                        Argb::PEARL_RING_USER_CAPTURE,
                        BlinkDurations::from(vec![0.0, 0.6, 3.6]),
                        None,
                        false,
                    ),
                );
            }
            Event::Idle => {
                self.stop_ring(LEVEL_FOREGROUND, Transition::ForceStop);
                self.stop_center(LEVEL_FOREGROUND, Transition::ForceStop);
                self.stop_ring(LEVEL_NOTICE, Transition::ForceStop);
                self.stop_center(LEVEL_NOTICE, Transition::ForceStop);

                self.operator_signup_phase.idle();
                self.set_center(
                    LEVEL_BACKGROUND,
                    animations::Static::<PEARL_CENTER_LED_COUNT>::new(Argb::OFF, None),
                );
                self.set_ring(
                    LEVEL_FOREGROUND,
                    animations::Static::<PEARL_RING_LED_COUNT>::new(
                        Argb::PEARL_RING_USER_QR_SCAN,
                        None,
                    )
                    .fade_in(1.5),
                );
            }
            Event::VoiceOpenEyes => {
                self.sound.queue(
                    sound::Type::Voice(sound::Voice::OpenEyes),
                    Duration::ZERO,
                )?;
            }
            _ => {}
        }
        Ok(())
    }
}
