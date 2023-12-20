use crate::engine;
use crate::engine::rgb::Argb;
use crate::engine::{AnimationState, OperatorFrame};
use std::{any::Any, f64::consts::PI};

use super::Animation;

const PULSE_SPEED: f64 = PI / 1.5; // 1 seconds per pulse (sine 1->0->1)

#[derive(Clone, Copy, Debug, PartialOrd, PartialEq)]
enum SignupProgress {
    /// start at 0 as it used as an index to build the color array
    SignupStarted = 0,
    OperatorQrCaptured = 1,
    UserQrCaptured = 2,
    IrisesCaptured = 3,
    IrisScanComplete = 4,
    Processing1 = 5,
    Processing2 = 6,
    BiometricPipelineSuccessful = 7,
    Uploading = 8,
    SignupSuccessful = 9,
}

#[derive(Clone, Copy, Default, Debug, PartialOrd, PartialEq)]
enum Phase {
    #[default]
    Idle,
    InProgress(SignupProgress),
    Failed(SignupProgress),
}

/// SignupPhase representation.
pub struct SignupPhase {
    orb_type: engine::OrbType,
    /// Signup current phase
    phase: Phase,
    /// Current phase, might be lagging behind `phase` as we want to display all the
    /// phases for a short time.
    current_phase: usize,
    color: [Argb; 5],
    time_since_last_changed: f64,
    warning_pulse_ph_rad: f64,
    capture_warning_flags: u32,
}

enum CaptureConditions {
    Occlusion = 1,
    Distance = 2,
    OperatorQrCode = 3,
    UserQrCode = 4,
}

impl SignupPhase {
    pub fn new(orb_type: engine::OrbType) -> Self {
        Self {
            orb_type,
            phase: Phase::Idle,
            current_phase: 0,
            color: [Argb::OFF; 5],
            time_since_last_changed: 0.0,
            warning_pulse_ph_rad: 0.0,
            capture_warning_flags: 0,
        }
    }

    /// Sets phase to idle.
    pub fn idle(&mut self) {
        self.phase = Phase::Idle;
    }

    /// Sets phase to capture started.
    pub fn signup_phase_started(&mut self) {
        self.phase = Phase::InProgress(SignupProgress::SignupStarted);
        self.current_phase = 0;
    }

    /// Sets phase to operator qr captured.
    pub fn operator_qr_captured(&mut self) {
        self.phase = Phase::InProgress(SignupProgress::OperatorQrCaptured);
    }

    /// Indicates that operator qr code is wrong
    pub fn operator_qr_code_issue(&mut self) {
        if self.warning_pulse_ph_rad <= 0.0 {
            self.warning_pulse_ph_rad = PI;
        }
        self.capture_warning_flags |= 1 << CaptureConditions::OperatorQrCode as usize;
    }

    /// Indicates that a new operator qr code is to be scanned
    pub fn operator_qr_code_ok(&mut self) {
        self.capture_warning_flags &=
            !(1 << CaptureConditions::OperatorQrCode as usize);
    }

    /// Sets phase to user qr captured.
    pub fn user_qr_captured(&mut self) {
        self.phase = Phase::InProgress(SignupProgress::UserQrCaptured);
    }

    /// Indicates that user qr code is wrong
    pub fn user_qr_code_issue(&mut self) {
        if self.warning_pulse_ph_rad <= 0.0 {
            self.warning_pulse_ph_rad = PI;
        }
        self.capture_warning_flags |= 1 << CaptureConditions::UserQrCode as usize;
    }

    /// Indicates that a new user qr code is to be scanned
    pub fn user_qr_code_ok(&mut self) {
        self.capture_warning_flags &= !(1 << CaptureConditions::UserQrCode as usize);
    }

    /// Sets phase to irises captured.
    pub fn irises_captured(&mut self) {
        self.phase = Phase::InProgress(SignupProgress::IrisesCaptured);
    }

    /// Indicates that capture prevented by eye occlusion
    pub fn capture_occlusion_issue(&mut self) {
        if self.warning_pulse_ph_rad <= 0.0 {
            self.warning_pulse_ph_rad = PI;
        }
        self.capture_warning_flags |= 1 << CaptureConditions::Occlusion as usize;
    }

    /// Indicates that capture can be performed thanks to open eyes
    pub fn capture_occlusion_ok(&mut self) {
        self.capture_warning_flags &= !(1 << CaptureConditions::Occlusion as usize);
    }

    /// Indicates that capture prevented by distance to user
    pub fn capture_distance_issue(&mut self) {
        if self.warning_pulse_ph_rad <= 0.0 {
            self.warning_pulse_ph_rad = PI;
        }
        self.capture_warning_flags |= 1 << CaptureConditions::Distance as usize;
    }

    /// Indicates that capture can be performed thanks to a correct distance to user
    pub fn capture_distance_ok(&mut self) {
        self.capture_warning_flags &= !(1 << CaptureConditions::Distance as usize);
    }

    /// Sets phase to iris scan complete.
    pub fn iris_scan_complete(&mut self) {
        self.phase = Phase::InProgress(SignupProgress::IrisScanComplete);
    }

    /// Sets phase to processing 1.
    pub fn processing_1(&mut self) {
        self.phase = Phase::InProgress(SignupProgress::Processing1);
    }

    /// Sets phase to processing 2.
    pub fn processing_2(&mut self) {
        // processing_2 is received multiple times even when next phases are reached
        // so don't change phase if we are already in Processing2 or after
        if let Phase::InProgress(SignupProgress::Processing1) = self.phase {
            self.phase = Phase::InProgress(SignupProgress::Processing2);
        }
    }

    /// Sets phase to successful biometric pipeline.
    pub fn biometric_pipeline_successful(&mut self) {
        self.phase = Phase::InProgress(SignupProgress::BiometricPipelineSuccessful);
    }

    /// Sets phase to uploading.
    pub fn uploading(&mut self) {
        self.phase = Phase::InProgress(SignupProgress::Uploading);
    }

    /// Sets phase to successful signup
    pub fn signup_successful(&mut self) {
        self.phase = Phase::InProgress(SignupProgress::SignupSuccessful);
    }

    /// Sets failed.
    pub fn failure(&mut self) {
        if let Phase::InProgress(phase) = self.phase {
            tracing::debug!("Signup failed at phase {phase:?}");
            self.phase = Phase::Failed(phase);
        }
    }
}

impl Animation for SignupPhase {
    type Frame = OperatorFrame;

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn animate(
        &mut self,
        frame: &mut OperatorFrame,
        dt: f64,
        idle: bool,
    ) -> AnimationState {
        // filter out fast changing states by only updating the animation every 0.5 second
        self.time_since_last_changed += dt;
        if self.time_since_last_changed > 0.5 {
            match self.phase {
                Phase::Failed(failed_phase) => {
                    self.color = [Argb::OFF; 5];
                    self.color
                        .iter_mut()
                        .rev()
                        .enumerate()
                        .take_while(|(i, _)| *i <= (failed_phase as usize % 5))
                        .for_each(|(_, c)| {
                            *c = match self.orb_type {
                                engine::OrbType::Pearl => Argb::PEARL_OPERATOR_AMBER,
                                engine::OrbType::Diamond => {
                                    Argb::DIAMOND_OPERATOR_AMBER
                                }
                            };
                        });
                }
                Phase::InProgress(phase) => {
                    if self.current_phase < phase as usize {
                        // advance to next step one by one
                        self.current_phase += 1;
                        self.time_since_last_changed = 0.0;
                        self.warning_pulse_ph_rad = 0.0;
                    }

                    self.color = [Argb::OFF; 5];
                    self.color
                        .iter_mut()
                        .rev()
                        .enumerate()
                        .take_while(|(i, _)| *i <= (self.current_phase % 5))
                        .for_each(|(_, c)| {
                            *c = match self.orb_type {
                                engine::OrbType::Pearl => Argb::PEARL_OPERATOR_DEFAULT,
                                engine::OrbType::Diamond => {
                                    Argb::DIAMOND_OPERATOR_DEFAULT
                                }
                            };
                        });
                }
                Phase::Idle => {}
            }
        }

        if !idle && !matches!(self.phase, Phase::Idle) {
            let progress = match self.phase {
                Phase::Idle => {
                    unreachable!();
                }
                Phase::InProgress(x) | Phase::Failed(x) => x,
            };

            if self.warning_pulse_ph_rad > 0.0
                && !self.color[progress as usize % 5].is_off()
            {
                let mut animated_frame = self.color;
                // go through LED from the last to the first in the array (from right to left
                // when looking at the device from the front) and get the last colorized LED
                // to animate it
                if let Some((i, _)) = self
                    .color
                    .iter()
                    .enumerate()
                    .rev()
                    .filter(|(_, &c)| !c.is_off())
                    .last()
                {
                    let color = match self.orb_type {
                        engine::OrbType::Pearl => Argb::PEARL_OPERATOR_AMBER,
                        engine::OrbType::Diamond => Argb::DIAMOND_OPERATOR_AMBER,
                    };
                    animated_frame[i] = color * self.warning_pulse_ph_rad.sin();
                }
                // wait for warning animation to finish before we either restart
                // the animation or end it if no warning set
                if self.warning_pulse_ph_rad - dt * PULSE_SPEED <= 0.0 {
                    if self.capture_warning_flags == 0 {
                        self.warning_pulse_ph_rad = 0.0;
                    } else {
                        self.warning_pulse_ph_rad =
                            PI - (self.warning_pulse_ph_rad - dt * PULSE_SPEED);
                    }
                } else {
                    self.warning_pulse_ph_rad -= dt * PULSE_SPEED;
                }
                frame.copy_from_slice(&animated_frame);
            } else {
                frame.copy_from_slice(&self.color);
            }
        }
        AnimationState::Running
    }
}
