use std::any::Any;
use crate::engine::Animation;
use crate::engine::{AnimationState, PEARL_CENTER_LED_COUNT};
use orb_rgb::Argb;
use std::f64::consts::PI;

/// Pulsing wave animation.
/// Starts with a solid `color` or off (`inverted`), then fades to its contrary and loops.
pub struct Wave<const N: usize> {
    color: Argb,
    wave_period: f64,
    solid_period: f64,
    inverted: bool,
    phase: f64,
    total_period: f64,
}

impl<const N: usize> Wave<N> {
    /// Creates a new [`Wave`].
    #[must_use]
    pub fn new(
        color: Argb,
        wave_period: f64,
        solid_period: f64,
        inverted: bool,
    ) -> Self {
        let total_period = wave_period + solid_period;
        Self {
            color,
            wave_period,
            solid_period,
            inverted,
            phase: 0.0,
            total_period,
        }
    }
}

impl<const N: usize> Animation for Wave<N> {
    type Frame = [Argb; N];

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
        frame: &mut [Argb; N],
        dt: f64,
        idle: bool,
    ) -> AnimationState {
        self.phase += dt;
        self.phase %= self.total_period;

        if !idle {
            let intensity = if self.phase < self.solid_period {
                // Solid period
                if self.inverted { 0.0 } else { 1.0 }
            } else {
                // Wave period
                let wave_phase = (self.phase - self.solid_period) / self.wave_period * 2.0 * PI;
                if self.inverted {
                    (1.0 - wave_phase.cos()) / 2.0
                } else {
                    (1.0 + wave_phase.cos()) / 2.0
                }
            };

            // Pure dimming effect
            let dimmed_color = Argb(
                self.color.0,
                (f64::from(self.color.1) * intensity) as u8,
                (f64::from(self.color.2) * intensity) as u8,
                (f64::from(self.color.3) * intensity) as u8,
            );

            if N == PEARL_CENTER_LED_COUNT {
                // For pearl center LEDs, distribute the dimmed color
                for (i, led) in frame.iter_mut().enumerate() {
                    // Convert linear indexing into a spiral:
                    // 6 7 8
                    // 5 0 1
                    // 4 3 2
                    const SPIRAL: [usize; 9] = [6, 7, 8, 5, 0, 1, 4, 3, 2];
                    let j = SPIRAL[i];
                    let r = if j <= (dimmed_color.1 as usize * N / 255) { dimmed_color.1 } else { dimmed_color.1.saturating_sub(1) };
                    let g = if j <= (dimmed_color.2 as usize * N / 255) { dimmed_color.2 } else { dimmed_color.2.saturating_sub(1) };
                    let b = if j <= (dimmed_color.3 as usize * N / 255) { dimmed_color.3 } else { dimmed_color.3.saturating_sub(1) };
                    *led = Argb(dimmed_color.0, r, g, b);
                }
            } else {
                // For non-pearl center LEDs, apply uniform dimming
                for led in frame.iter_mut() {
                    *led = dimmed_color;
                }
            }
        }
        AnimationState::Running
    }

    fn transition_from(&mut self, _superseded: &dyn Any) {
        self.phase = 0.0;
    }
}