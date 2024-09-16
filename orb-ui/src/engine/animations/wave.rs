use crate::engine::Animation;
use crate::engine::{AnimationState, PEARL_CENTER_LED_COUNT};
use orb_rgb::Argb;
use std::{any::Any, f64::consts::PI};

/// Pulsing wave animation.
/// Starts with a solid `color` or off (`inverted`), then fades to its contrary and loops.
pub struct Wave<const N: usize> {
    color: Argb,
    wave_period: f64,
    solid_period: f64,
    start_off: bool,
    phase: f64,
    initial_delay: f64,
    repeat: Option<usize>,
}

impl<const N: usize> Wave<N> {
    /// Creates a new [`Wave`].
    /// By default, infinite loop, no delay
    #[must_use]
    pub fn new(
        color: Argb,
        wave_period: f64,
        solid_period: f64,
        start_off: bool,
    ) -> Self {
        Self {
            color,
            wave_period,
            solid_period,
            start_off,
            phase: 0.0,
            initial_delay: 0.0,
            repeat: None, // infinite
        }
    }

    pub fn with_delay(mut self, delay: f64) -> Self {
        self.initial_delay = delay;
        self
    }

    #[expect(dead_code)]
    pub fn repeat(mut self, n_times: usize) -> Self {
        self.repeat = Some(n_times);
        self
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
        if self.initial_delay > 0.0 {
            self.initial_delay -= dt;
            return AnimationState::Running;
        }

        if self.phase >= self.solid_period {
            self.phase += dt * (PI * 2.0 / self.wave_period);
        } else {
            self.phase += dt;
        }

        // check if at the end of the animation, if phase wraps around
        if let Some(repeat) = self.repeat.as_mut() {
            if self.phase % (PI * 2.0 + self.solid_period) < self.phase {
                if *repeat > 0 {
                    *repeat -= 1;
                }
                if *repeat == 0 {
                    return AnimationState::Finished;
                }
            }
        }

        self.phase %= PI * 2.0 + self.solid_period;
        if !idle {
            if self.phase >= self.solid_period {
                let intensity = if self.start_off {
                    // starts at intensity 0
                    (1.0 - (self.phase - self.solid_period).cos()) / 2.0
                } else {
                    // starts at intensity 1
                    ((self.phase - self.solid_period).cos() + 1.0) / 2.0
                };

                if N == PEARL_CENTER_LED_COUNT {
                    let r = f64::from(self.color.1) * intensity;
                    let g = f64::from(self.color.2) * intensity;
                    let b = f64::from(self.color.3) * intensity;

                    let r_low = r.floor() as u8;
                    let r_high = r.ceil() as u8;
                    let r_count = (r.fract() * N as f64) as usize;
                    let g_low = g.floor() as u8;
                    let g_high = g.ceil() as u8;
                    let g_count = (g.fract() * N as f64) as usize;
                    let b_low = b.floor() as u8;
                    let b_high = b.ceil() as u8;
                    let b_count = (b.fract() * N as f64) as usize;
                    for (i, led) in frame.iter_mut().enumerate() {
                        // Convert linear indexing into a spiral:
                        // 6 7 8
                        // 5 0 1
                        // 4 3 2
                        const SPIRAL: [usize; 9] = [6, 7, 8, 5, 0, 1, 4, 3, 2];
                        let j = SPIRAL[i];
                        let r = if j <= r_count { r_high } else { r_low };
                        let g = if j <= g_count { g_high } else { g_low };
                        let b = if j <= b_count { b_high } else { b_low };

                        *led = Argb(None, r, g, b);
                    }
                } else {
                    // diamond
                    for led in frame.iter_mut() {
                        *led = self.color * intensity;
                    }
                }
            } else {
                for led in &mut *frame {
                    if self.start_off {
                        *led = Argb::OFF;
                    } else {
                        *led = self.color;
                    }
                }
            }
        }
        AnimationState::Running
    }

    // stop at the end of the animation
    fn stop(&mut self) {
        self.repeat = Some(1);
    }

    fn transition_from(&mut self, _superseded: &dyn Any) {
        self.phase = 0.0;
    }
}
