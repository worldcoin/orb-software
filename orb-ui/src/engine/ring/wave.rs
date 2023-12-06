use super::Animation;
use crate::engine::rgb::Rgb;
use crate::engine::{AnimationState, RingFrame};
use std::{any::Any, f64::consts::PI};

/// Pulsing wave animation.
/// Starts with a solid `color` or off (`inverted`), then fades to its contrary and loops.
pub struct Wave<const N: usize> {
    color: Rgb,
    wave_period: f64,
    solid_period: f64,
    inverted: bool,
    phase: f64,
}

impl<const N: usize> Wave<N> {
    /// Creates a new [`Wave`].
    #[must_use]
    pub fn new(
        color: Rgb,
        wave_period: f64,
        solid_period: f64,
        inverted: bool,
    ) -> Self {
        Self {
            color,
            wave_period,
            solid_period,
            inverted,
            phase: 0.0,
        }
    }
}

impl<const N: usize> Animation for Wave<N> {
    type Frame = RingFrame<N>;

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    fn animate(
        &mut self,
        frame: &mut RingFrame<N>,
        dt: f64,
        idle: bool,
    ) -> AnimationState {
        if self.phase >= self.solid_period {
            self.phase += dt * (PI * 2.0 / self.wave_period);
        } else {
            self.phase += dt;
        }
        self.phase %= PI * 2.0 + self.solid_period;
        if !idle {
            if self.phase >= self.solid_period {
                let intensity = if self.inverted {
                    // starts at intensity 0
                    (1.0 - (self.phase - self.solid_period).cos()) / 2.0
                } else {
                    // starts at intensity 1
                    ((self.phase - self.solid_period).cos() + 1.0) / 2.0
                };
                let r = f64::from(self.color.0) * intensity;
                let g = f64::from(self.color.1) * intensity;
                let b = f64::from(self.color.2) * intensity;
                for led in frame.iter_mut() {
                    *led = Rgb(r as u8, g as u8, b as u8);
                }
            } else {
                for led in &mut *frame {
                    if self.inverted {
                        *led = Rgb(0, 0, 0);
                    } else {
                        *led = self.color;
                    }
                }
            }
        }
        AnimationState::Running
    }

    fn transition_from(&mut self, _superseded: &dyn Any) {
        self.phase = 0.0;
    }
}
