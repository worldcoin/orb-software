use super::Animation;
use crate::engine;
use crate::engine::{AnimationState, OperatorFrame, Transition};
use eyre::eyre;
use orb_rgb::Argb;
use std::{any::Any, f64::consts::PI};

/// Pulse with all LEDs.
#[derive(Default)]
pub struct Pulse {
    orb_type: engine::OrbType,
    /// period of the wave, set to 0.0 to hide the animation/pulses
    wave_period: f64,
    solid_period: f64,
    inverted: bool,
    phase: f64,
    transition: Option<Transition>,
    transition_time: f64,
    color: Argb,
}

impl Pulse {
    pub fn new(orb_type: engine::OrbType) -> Self {
        Self {
            orb_type,
            ..Default::default()
        }
    }

    /// Start a new pulse sequence.
    pub fn trigger(
        &mut self,
        wave_period: f64,
        solid_period: f64,
        inverted: bool,
        api_mode: bool,
    ) {
        self.color = if api_mode {
            Argb::OPERATOR_DEV
        } else {
            match self.orb_type {
                engine::OrbType::Pearl => Argb::PEARL_OPERATOR_DEFAULT,
                engine::OrbType::Diamond => Argb::DIAMOND_OPERATOR_DEFAULT,
            }
        };
        self.wave_period = wave_period;
        self.solid_period = solid_period;
        self.inverted = inverted;
        self.phase = 0.0;
    }
}

impl Animation for Pulse {
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
        if self.wave_period == 0.0 {
            return AnimationState::Finished;
        } else if let Some(Transition::ForceStop) = self.transition {
            self.wave_period = 0.0;
            return AnimationState::Finished;
        } else if let Some(Transition::StartDelay(duration)) = self.transition {
            self.transition_time += dt;
            if self.transition_time >= duration {
                self.transition = None;
            }
            return AnimationState::Running;
        }

        if self.phase >= self.solid_period && self.wave_period != 0.0 {
            self.phase += dt * (PI * 2.0 / self.wave_period);
        } else {
            self.phase += dt;
        }
        self.phase %= PI * 2.0 + self.solid_period;
        if !idle {
            let color = if self.phase >= self.solid_period {
                let intensity = if self.inverted {
                    // starts at intensity 0
                    (1.0 - (self.phase - self.solid_period).cos()) / 2.0
                } else {
                    // starts at intensity 1
                    ((self.phase - self.solid_period).cos() + 1.0) / 2.0
                };
                if let Some(Transition::PlayOnce) = self.transition {
                    if intensity < 0.1 {
                        // turn animatin off
                        self.wave_period = 0.0;
                        return AnimationState::Finished;
                    }
                }
                self.color * intensity
            } else {
                // solid
                self.color
            };
            for led in frame {
                *led = color;
            }
        }
        AnimationState::Running
    }

    fn stop(&mut self, transition: Transition) -> eyre::Result<()> {
        match transition {
            Transition::Shrink | Transition::FadeIn(_) | Transition::FadeOut(_) => {
                return Err(eyre!(
                    "Transition {:?} not supported for pulse animation",
                    transition
                ));
            }
            t => {
                self.transition = Some(t);
                self.transition_time = 0.0;
            }
        }

        Ok(())
    }
}
