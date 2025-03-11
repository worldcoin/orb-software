use crate::engine::animations::{SimpleSpinner, Static};
use crate::engine::{Animation, Transition, TransitionStatus};
use crate::engine::{AnimationState, PEARL_CENTER_LED_COUNT};
use eyre::eyre;
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
    repeat: Option<usize>,
    transition: Option<Transition>,
    transition_color: Option<Argb>,
    transition_time: f64,
    min_color_intensity: Option<Argb>,
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
        min_color_intensity: Option<Argb>,
    ) -> Self {
        Self {
            color,
            wave_period,
            solid_period,
            start_off,
            phase: 0.0,
            repeat: None, // infinite
            transition: None,
            transition_color: None,
            transition_time: 0.0,
            min_color_intensity,
        }
    }

    pub fn with_delay(mut self, delay: f64) -> Self {
        self.transition = Some(Transition::StartDelay(delay));
        self
    }

    pub fn fade_in(mut self, duration: f64) -> Self {
        self.transition = Some(Transition::FadeIn(duration));
        self
    }

    #[expect(dead_code)]
    pub fn repeat(mut self, n_times: usize) -> Self {
        self.repeat = Some(n_times);
        self
    }

    pub fn color(&self) -> Argb {
        self.color
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
        if let Some(Transition::ForceStop) = self.transition {
            return AnimationState::Finished;
        }

        if let Some(Transition::StartDelay(delay)) = self.transition {
            self.transition_time += dt;
            if self.transition_time >= delay {
                self.transition = None;
            } else {
                return AnimationState::Running;
            }
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

        // compute any transition color
        let scaling_factor = match self.transition {
            Some(Transition::FadeOut(duration)) => {
                self.transition_time += dt;
                if self.transition_time >= duration {
                    return AnimationState::Finished;
                }
                (self.transition_time * PI / 2.0 / duration).cos()
            }
            Some(Transition::FadeIn(duration)) => {
                self.transition_time += dt;
                if self.transition_time >= duration {
                    self.transition = None;
                }
                (self.transition_time * PI / 2.0 / duration).sin()
            }
            _ => 1.0,
        };

        // average between color and transition_background
        let mut color = if let Some(transition_color) = self.transition_color {
            let mut color = transition_color.lerp(self.color, scaling_factor);
            // use target dimming value if transitioning from an Argb value without a dimming value
            if color.0 == Some(0) && color.0 != self.color.0 {
                color.0 = self.color.0;
            }
            color
        } else {
            self.color * scaling_factor
        };

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

                // specific case for pearl center
                if N == PEARL_CENTER_LED_COUNT {
                    let r = f64::from(color.1) * intensity;
                    let g = f64::from(color.2) * intensity;
                    let b = f64::from(color.3) * intensity;

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
                    // pearl's ring or diamond

                    // clamp color intensity to self.min_color_intensity
                    color *= intensity;
                    if let Some(min_color_intensity) = self.min_color_intensity {
                        color.1 = color.1.max(min_color_intensity.1);
                        color.2 = color.2.max(min_color_intensity.2);
                        color.3 = color.3.max(min_color_intensity.3);
                    }

                    for led in frame.iter_mut() {
                        *led = color;
                    }
                }
            } else {
                for led in &mut *frame {
                    if self.start_off {
                        *led = Argb::OFF;
                    } else {
                        *led = color;
                    }
                }
            }
        }
        AnimationState::Running
    }

    fn transition_from(&mut self, superseded: &dyn Any) -> TransitionStatus {
        if let Some(simple_spinner) = superseded.downcast_ref::<SimpleSpinner<N>>() {
            self.phase = 0.0;
            self.transition_color = Some(simple_spinner.background());
            self.transition_time = 0.0;
            TransitionStatus::Smooth
        } else if let Some(static_animation) = superseded.downcast_ref::<Static<N>>() {
            self.phase = 0.0;
            self.transition_color = Some(static_animation.color());
            self.transition_time = 0.0;
            TransitionStatus::Smooth
        } else if let Some(wave_animation) = superseded.downcast_ref::<Wave<N>>() {
            self.phase = 0.0;
            self.transition_color = Some(wave_animation.color());
            self.transition_time = 0.0;
            TransitionStatus::Smooth
        } else {
            TransitionStatus::Sharp
        }
    }

    // stop at the end of the animation
    fn stop(&mut self, transition: Transition) -> eyre::Result<()> {
        if let Transition::PlayOnce = transition {
            self.repeat = Some(1);
            self.transition = None;
        } else if transition == Transition::Shrink {
            return Err(eyre!(
                "Transition {:?} not supported for wave animation",
                transition
            ));
        } else {
            self.transition_color = Some(Argb::OFF);
            self.transition = Some(transition);
            self.transition_time = 0.0;
        }

        Ok(())
    }
}
