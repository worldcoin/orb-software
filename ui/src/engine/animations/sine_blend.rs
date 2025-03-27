use crate::engine::animations::{SimpleSpinner, Static};
use crate::engine::{Animation, Transition, TransitionStatus};
use crate::engine::{AnimationState, PEARL_CENTER_LED_COUNT};
use eyre::eyre;
use orb_rgb::Argb;
use std::{any::Any, f64::consts::PI};

use super::Wave;

/// Pulsing wave animation between two colors.
pub struct SineBlend<const N: usize> {
    color1: Argb,
    color2: Argb,
    wave_period: f64,
    solid_period: f64,
    phase: f64,
    repeat: Option<usize>,
    transition: Option<Transition>,
    transition_color: Option<Argb>,
    transition_time: f64,
}

impl<const N: usize> SineBlend<N> {
    /// Creates a new [`Wave`].
    /// By default, infinite loop, no delay
    #[must_use]
    pub fn new(
        color1: Argb,
        color2: Argb,
        wave_period: f64,
        solid_period: f64,
    ) -> Self {
        Self {
            color1,
            color2,
            wave_period,
            solid_period,
            phase: 0.0,
            repeat: None, // infinite
            transition: None,
            transition_color: None,
            transition_time: 0.0,
        }
    }

    fn fill_frame(frame: &mut [Argb; N], color: Argb, idle: bool) {
        if !idle {
            // specific case for pearl center
            if N == PEARL_CENTER_LED_COUNT {
                let r = f64::from(color.1);
                let g = f64::from(color.2);
                let b = f64::from(color.3);

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
                for led in &mut *frame {
                    *led = color;
                }
            }
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
}

impl<const N: usize> Animation for SineBlend<N> {
    type Frame = [Argb; N];

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

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

        if self.solid_period > 0.0 {
            self.solid_period -= dt;
            SineBlend::fill_frame(frame, self.color1, idle);
        }

        let t = 0.5 * (1.0 - self.phase.cos());
        let color = self.color1.lerp(self.color2, t);
        SineBlend::fill_frame(frame, color, idle);

        self.phase += dt * (2.0 * PI / self.wave_period);

        AnimationState::Running
    }

    fn transition_from(&mut self, superseded: &dyn Any) -> TransitionStatus {
        if let Some(simple_spinner) = superseded.downcast_ref::<SimpleSpinner<N>>() {
            self.phase = 0.0;
            self.transition_time = 0.0;
            self.transition_color = Some(simple_spinner.background());
            TransitionStatus::Smooth
        } else if let Some(static_animation) = superseded.downcast_ref::<Static<N>>() {
            self.phase = 0.0;
            self.transition_time = 0.0;
            self.transition_color = Some(static_animation.color());
            TransitionStatus::Smooth
        } else if let Some(wave_animation) = superseded.downcast_ref::<Wave<N>>() {
            self.phase = 0.0;
            self.transition_time = 0.0;
            self.transition_color = Some(wave_animation.color());
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
