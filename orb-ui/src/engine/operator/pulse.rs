use super::Animation;
use crate::engine;
use crate::engine::rgb::Argb;
use crate::engine::{AnimationState, OperatorFrame, OrbType};
use std::{any::Any, f64::consts::PI};

/// Pulse with all LEDs.
#[derive(Default)]
pub struct Pulse {
    orb_type: engine::OrbType,
    wave_period: f64,
    solid_period: f64,
    inverted: bool,
    duration: f64,
    phase: Option<f64>,
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
        duration: f64,
        wave_period: f64,
        solid_period: f64,
        inverted: bool,
    ) {
        self.wave_period = wave_period;
        self.solid_period = solid_period;
        self.inverted = inverted;
        self.duration = duration;
        self.phase = Some(0.0);
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
        if let Some(phase) = self.phase.as_mut() {
            if *phase >= self.solid_period {
                *phase += dt * (PI * 2.0 / self.wave_period);
            } else {
                *phase += dt;
            }
            *phase %= PI * 2.0 + self.solid_period;
            if !idle {
                let color = if *phase >= self.solid_period {
                    let intensity = if self.inverted {
                        // starts at intensity 0
                        (1.0 - (*phase - self.solid_period).cos()) / 2.0
                    } else {
                        // starts at intensity 1
                        ((*phase - self.solid_period).cos() + 1.0) / 2.0
                    };
                    match self.orb_type {
                        OrbType::Pearl => Argb::PEARL_OPERATOR_DEFAULT * intensity,
                        OrbType::Diamond => Argb::DIAMOND_OPERATOR_DEFAULT * intensity,
                    }
                } else {
                    // solid
                    match self.orb_type {
                        OrbType::Pearl => Argb::PEARL_OPERATOR_DEFAULT,
                        OrbType::Diamond => Argb::DIAMOND_OPERATOR_DEFAULT,
                    }
                };
                for led in frame {
                    *led = color;
                }
            }
        }
        AnimationState::Running
    }

    fn stop(&mut self) {
        *self = Self::default();
    }
}
