use crate::engine::animations::alert::BlinkDurations;
use crate::engine::{
    animations, Animation, Event, QrScanSchema, QrScanUnexpectedReason, Runner,
    RunningAnimation, SignupFailReason, Transition, UiMode, UiState,
    BIOMETRIC_PIPELINE_MAX_PROGRESS, LEVEL_BACKGROUND, LEVEL_FOREGROUND, LEVEL_NOTICE,
    PEARL_CENTER_LED_COUNT, PEARL_RING_LED_COUNT,
};
use crate::sound;
use crate::sound::Player;
use eyre::Result;
use orb_rgb::Argb;
use std::f64::consts::PI;
use std::time::Duration;

impl Runner<PEARL_RING_LED_COUNT, PEARL_CENTER_LED_COUNT> {
    pub(super) fn event_operator(&mut self, event: &Event) -> Result<()> {
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
                self.state =
                    UiState::Booted(if *api_mode { UiMode::Api } else { UiMode::Core });
            }
            Event::Shutdown { requested } => {
                self.sound.queue(
                    sound::Type::Melody(sound::Melody::PoweringDown),
                    Duration::ZERO,
                )?;
                // overwrite any existing animation by setting notice-level animation
                // as the last animation before shutdown
                self.set_center(
                    LEVEL_NOTICE,
                    animations::Alert::<PEARL_CENTER_LED_COUNT>::new(
                        if *requested {
                            Argb::PEARL_USER_QR_SCAN
                        } else {
                            Argb::PEARL_USER_AMBER
                        },
                        BlinkDurations::from(vec![0.0, 0.3, 0.45, 0.3, 0.45, 0.45]),
                        None,
                        false,
                    )?,
                );
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
                    crate::engine::pearl_operator_default(),
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
                self.stop_ring(LEVEL_FOREGROUND, Transition::ForceStop);
                self.stop_center(LEVEL_FOREGROUND, Transition::ForceStop);
                self.stop_center(LEVEL_NOTICE, Transition::ForceStop);

                // reset ring background to black/off so that it's turned off in next animations
                self.set_ring(
                    LEVEL_BACKGROUND,
                    animations::Static::<PEARL_RING_LED_COUNT>::new(Argb::OFF, None),
                );
            }
            Event::QrScanStart { schema } => {
                self.set_center(
                    LEVEL_FOREGROUND,
                    animations::Wave::<PEARL_CENTER_LED_COUNT>::new(
                        Argb::PEARL_USER_QR_SCAN,
                        5.0,
                        0.5,
                        true,
                        None,
                    ),
                );

                match schema {
                    QrScanSchema::Operator | QrScanSchema::OperatorSelfServe => {
                        self.operator_signup_phase.operator_qr_code_ok();
                    }
                    QrScanSchema::Wifi => {
                        self.operator_idle.no_wlan();

                        // temporarily increase the volume to ask wifi qr code
                        let master_volume = self.sound.volume();
                        self.sound.set_master_volume(30);
                        self.sound.queue(
                            sound::Type::Voice(sound::Voice::ShowWifiHotspotQrCode),
                            Duration::ZERO,
                        )?;
                        self.sound.set_master_volume(master_volume);
                    }
                    QrScanSchema::User => {
                        self.operator_signup_phase.user_qr_code_ok();
                        // initialize ring with short segment to invite user to scan QR
                        self.set_ring(
                            LEVEL_FOREGROUND,
                            animations::Slider::<PEARL_RING_LED_COUNT>::new(
                                0.0,
                                Argb::PEARL_USER_SIGNUP,
                            ),
                        );
                    }
                };
            }
            Event::QrScanCapture => {
                // stop wave (foreground) & show alert/blinks (notice)
                self.stop_center(LEVEL_FOREGROUND, Transition::ForceStop);
                self.sound.queue(
                    sound::Type::Melody(sound::Melody::QrCodeCapture),
                    Duration::ZERO,
                )?;
            }
            Event::QrScanCompleted { schema: _ } => {
                // stop wave (foreground) & show alert/blinks (notice)
                self.stop_center(LEVEL_FOREGROUND, Transition::ForceStop);
                self.set_center(
                    LEVEL_NOTICE,
                    animations::Alert::<PEARL_CENTER_LED_COUNT>::new(
                        Argb::PEARL_USER_QR_SCAN,
                        BlinkDurations::from(vec![0.0, 0.3, 0.45, 0.46]),
                        None,
                        false,
                    )?,
                );
            }
            Event::QrScanUnexpected { schema, reason } => {
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
                        // remove short segment from ring
                        self.stop_ring(LEVEL_FOREGROUND, Transition::ForceStop);
                        self.operator_signup_phase.user_qr_code_issue();
                    }
                    QrScanSchema::Operator | QrScanSchema::OperatorSelfServe => {
                        self.operator_signup_phase.operator_qr_code_issue();
                    }
                    QrScanSchema::Wifi => {}
                }
                // stop wave
                self.stop_center(LEVEL_FOREGROUND, Transition::ForceStop);
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
                        // in case schema is user qr
                        self.stop_ring(LEVEL_FOREGROUND, Transition::ForceStop);
                        self.operator_signup_phase.failure();
                    }
                    QrScanSchema::Wifi => {}
                }
                // stop wave
                self.stop_center(LEVEL_FOREGROUND, Transition::ForceStop);
            }
            Event::QrScanSuccess { schema } => {
                match schema {
                    QrScanSchema::Operator | QrScanSchema::OperatorSelfServe => {
                        self.sound.queue(
                            sound::Type::Melody(sound::Melody::QrLoadSuccess),
                            Duration::ZERO,
                        )?;
                        self.operator_signup_phase.operator_qr_captured();
                    }
                    QrScanSchema::User => {
                        self.operator_signup_phase.user_qr_captured();
                        // see `Event::BiometricCaptureStart
                    }
                    QrScanSchema::Wifi => {
                        self.sound.queue(
                            sound::Type::Melody(sound::Melody::QrLoadSuccess),
                            Duration::ZERO,
                        )?;
                    }
                }
            }
            Event::QrScanTimeout { schema } => {
                self.sound
                    .queue(sound::Type::Voice(sound::Voice::Timeout), Duration::ZERO)?;
                match schema {
                    QrScanSchema::User
                    | QrScanSchema::Operator
                    | QrScanSchema::OperatorSelfServe => {
                        // in case schema is user qr
                        self.stop_ring(LEVEL_FOREGROUND, Transition::ForceStop);
                        self.operator_signup_phase.failure();
                    }
                    QrScanSchema::Wifi => {}
                }
                // stop wave
                self.stop_center(LEVEL_FOREGROUND, Transition::ForceStop);
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
                self.sound.queue(
                    sound::Type::Melody(sound::Melody::UserQrLoadSuccess),
                    Duration::ZERO,
                )?;
                // initialize ring with animated short segment to invite user to start iris capture
                self.set_ring(
                    LEVEL_NOTICE,
                    animations::Slider::<PEARL_RING_LED_COUNT>::new(
                        0.0,
                        Argb::PEARL_USER_SIGNUP,
                    )
                    .with_pulsing(),
                );
                // off background for biometric-capture, which relies on LEVEL_NOTICE animations
                self.stop_center(LEVEL_FOREGROUND, Transition::ForceStop);
                self.set_center(
                    LEVEL_FOREGROUND,
                    animations::Static::<PEARL_CENTER_LED_COUNT>::new(Argb::OFF, None),
                );
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
                            .downcast_mut::<animations::Progress<PEARL_RING_LED_COUNT>>(
                            )
                    })
                    .is_none()
                {
                    // in case animation not yet initialized through user QR scan success event
                    // initialize ring with short segment to invite user to start iris capture
                    self.set_ring(
                        LEVEL_NOTICE,
                        animations::Progress::<PEARL_RING_LED_COUNT>::new(
                            0.0,
                            None,
                            Argb::PEARL_USER_SIGNUP,
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
                            .downcast_mut::<animations::Progress<PEARL_RING_LED_COUNT>>(
                            )
                    });
                if let Some(ring_progress) = ring_progress {
                    ring_progress.set_progress(*progress, None);
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
                    if let Some(melody) = self.capture_sound.peekable().peek()
                        && self.sound.try_queue(sound::Type::Melody(*melody))?
                    {
                        self.capture_sound.next();
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
                self.sound.queue(
                    sound::Type::Melody(sound::Melody::IrisScanSuccess),
                    Duration::ZERO,
                )?;
                // set ring to full circle based on previous progress animation
                // ring will be reset when biometric pipeline starts showing progress
                let _ = self
                    .ring_animations_stack
                    .stack
                    .get_mut(&LEVEL_NOTICE)
                    .and_then(|RunningAnimation { animation, .. }| {
                        animation
                            .as_any_mut()
                            .downcast_mut::<animations::Progress<PEARL_RING_LED_COUNT>>(
                            )
                    })
                    .map(|x| {
                        x.set_progress(2.0, None);
                    });
                self.stop_center(LEVEL_NOTICE, Transition::PlayOnce);
                self.stop_ring(LEVEL_NOTICE, Transition::PlayOnce);

                // preparing animation for biometric pipeline progress
                self.set_ring(
                    LEVEL_FOREGROUND,
                    animations::Progress::<PEARL_RING_LED_COUNT>::new(
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
                            .downcast_mut::<animations::Progress<PEARL_RING_LED_COUNT>>(
                            )
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
                            .downcast_mut::<animations::Progress<PEARL_RING_LED_COUNT>>(
                            )
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
                            .downcast_mut::<animations::Progress<PEARL_RING_LED_COUNT>>(
                            )
                    });
                if let Some(slider) = slider {
                    slider.set_progress(BIOMETRIC_PIPELINE_MAX_PROGRESS, None);
                }
                self.operator_signup_phase.biometric_pipeline_successful();
            }
            Event::SignupFail { reason } => {
                self.sound.queue(
                    sound::Type::Melody(sound::Melody::SoundError),
                    Duration::ZERO,
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
                    SignupFailReason::Aborted => {}
                }
                self.operator_signup_phase.failure();

                let slider = self
                    .ring_animations_stack
                    .stack
                    .get_mut(&LEVEL_FOREGROUND)
                    .and_then(|RunningAnimation { animation, .. }| {
                        animation
                            .as_any_mut()
                            .downcast_mut::<animations::Progress<PEARL_RING_LED_COUNT>>(
                            )
                    });
                if let Some(slider) = slider {
                    slider.set_progress(2.0, None);
                }
                self.stop_ring(LEVEL_FOREGROUND, Transition::PlayOnce);
                self.stop_ring(LEVEL_NOTICE, Transition::ForceStop);
                self.stop_center(LEVEL_FOREGROUND, Transition::ForceStop);
                self.set_ring(
                    LEVEL_FOREGROUND,
                    animations::Idle::<PEARL_RING_LED_COUNT>::new(
                        Some(Argb::PEARL_USER_SIGNUP),
                        Some(1.0),
                    ),
                );
            }
            Event::SignupSuccess => {
                self.sound.queue(
                    sound::Type::Melody(sound::Melody::SignupSuccess),
                    Duration::ZERO,
                )?;

                self.operator_signup_phase.signup_successful();

                let slider = self
                    .ring_animations_stack
                    .stack
                    .get_mut(&LEVEL_FOREGROUND)
                    .and_then(|RunningAnimation { animation, .. }| {
                        animation
                            .as_any_mut()
                            .downcast_mut::<animations::Progress<PEARL_RING_LED_COUNT>>(
                            )
                    });
                if let Some(slider) = slider {
                    slider.set_progress(2.0, None);
                }
                self.stop_ring(LEVEL_FOREGROUND, Transition::ForceStop);
                self.stop_ring(LEVEL_NOTICE, Transition::ForceStop);
                self.stop_center(LEVEL_FOREGROUND, Transition::ForceStop);
                self.set_ring(
                    LEVEL_FOREGROUND,
                    animations::Idle::<PEARL_RING_LED_COUNT>::new(
                        Some(Argb::PEARL_USER_SIGNUP),
                        Some(3.0),
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
                    animations::Static::<PEARL_CENTER_LED_COUNT>::new(Argb::OFF, None),
                );
                self.set_ring(
                    LEVEL_BACKGROUND,
                    animations::Static::<PEARL_RING_LED_COUNT>::new(Argb::OFF, None),
                );
            }
            _ => {}
        }
        Ok(())
    }
}
