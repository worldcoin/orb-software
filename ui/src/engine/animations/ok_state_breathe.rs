use crate::engine::{Animation, AnimationState, Transition};
use orb_rgb::Argb;
use std::{any::Any, f64::consts::PI};

/// Duration over which the color fades from `start_color` to `end_color`.
const COLOR_TRANSITION_DURATION: f64 = 0.42;
/// Duration of the bloom intensity spike after the color transition starts.
const BLOOM_DURATION: f64 = 0.70;
/// Full period of the slow breathing cycle that follows the bloom (in seconds).
const BREATHE_PERIOD: f64 = 1.04;

/// OK-state breathing animation: all LEDs transition from `start_color` to
/// `end_color`, bloom briefly, then breathe slowly at `end_color`.
///
/// Mirrors the `frameDone` phase of the HTML `startOkState` animation.
pub struct OkStateBreathe<const N: usize> {
    start_color: Argb,
    end_color: Argb,
    elapsed: f64,
    transition: Option<Transition>,
    transition_time: f64,
}

impl<const N: usize> OkStateBreathe<N> {
    pub fn new(start_color: Argb, end_color: Argb) -> Self {
        Self {
            start_color,
            end_color,
            elapsed: 0.0,
            transition: None,
            transition_time: 0.0,
        }
    }
}

impl<const N: usize> Animation for OkStateBreathe<N> {
    type Frame = [Argb; N];

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn animate(&mut self, frame: &mut [Argb; N], dt: f64, idle: bool) -> AnimationState {
        let scaling_factor = match self.transition {
            Some(Transition::ForceStop) => return AnimationState::Finished,
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

        self.elapsed += dt;
        let e = self.elapsed;

        // Smooth color transition over COLOR_TRANSITION_DURATION.
        let color_t = (e / COLOR_TRANSITION_DURATION).clamp(0.0, 1.0);
        let color = self.start_color.lerp(self.end_color, color_t);

        // Bloom spike followed by slow breathing.
        let intensity = if e < BLOOM_DURATION {
            let b = (e / BLOOM_DURATION * PI).sin();
            1.0 + 0.9 * b
        } else {
            let br = 0.5 + 0.5 * ((e - BLOOM_DURATION) * 2.0 * PI / BREATHE_PERIOD).sin();
            0.85 + 0.15 * br
        };

        if !idle {
            let output = color * (intensity * scaling_factor);
            for led in frame.iter_mut() {
                *led = output;
            }
        }

        AnimationState::Running
    }

    fn stop(&mut self, transition: Transition) -> eyre::Result<()> {
        self.transition = Some(transition);
        self.transition_time = 0.0;

        Ok(())
    }
}
