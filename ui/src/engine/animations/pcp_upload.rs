use crate::engine::{Animation, AnimationState, Transition, PEARL_RING_LED_COUNT};
use eyre::eyre;
use orb_rgb::Argb;
use std::any::Any;
use std::f64::consts::PI;

const SPIN_PERIOD_SECONDS: f64 = 1.1;
const COMET_SPAN_RAD: f64 = PI * 1.55;
const TRACK_INTENSITY: f64 = 0.07;
const COMET_MIN_INTENSITY: f64 = 0.05;

/// Faint circular track with a fast clockwise comet tail.
pub struct PcpUploadSpinner<const N: usize> {
    phase: f64,
    color: Argb,
    transition: Option<Transition>,
    transition_time: f64,
}

impl<const N: usize> PcpUploadSpinner<N> {
    #[must_use]
    pub fn new(color: Argb) -> Self {
        Self {
            phase: 0.0,
            color,
            transition: None,
            transition_time: 0.0,
        }
    }

    #[must_use]
    pub fn fade_in(mut self, duration: f64) -> Self {
        self.transition = Some(Transition::FadeIn(duration));
        self
    }

    fn clockwise_index_direction() -> f64 {
        if N == PEARL_RING_LED_COUNT {
            1.0
        } else {
            -1.0
        }
    }
}

impl<const N: usize> Animation for PcpUploadSpinner<N> {
    type Frame = [Argb; N];

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    #[allow(clippy::cast_precision_loss)]
    fn animate(
        &mut self,
        frame: &mut [Argb; N],
        dt: f64,
        idle: bool,
    ) -> AnimationState {
        let scaling_factor = match self.transition {
            Some(Transition::ForceStop) => return AnimationState::Finished,
            Some(Transition::StartDelay(duration)) => {
                self.transition_time += dt;
                if self.transition_time >= duration {
                    self.transition = None;
                }
                return AnimationState::Running;
            }
            Some(Transition::FadeIn(duration)) => {
                self.transition_time += dt;
                if self.transition_time >= duration {
                    self.transition = None;
                }
                (self.transition_time * PI / 2.0 / duration).sin()
            }
            Some(Transition::FadeOut(duration)) => {
                self.transition_time += dt;
                if self.transition_time >= duration {
                    return AnimationState::Finished;
                }
                (self.transition_time * PI / 2.0 / duration).cos()
            }
            _ => 1.0,
        };

        self.phase = (self.phase + dt * PI * 2.0 / SPIN_PERIOD_SECONDS) % (PI * 2.0);

        if !idle {
            let direction = Self::clockwise_index_direction();
            let head = N as f64 / 2.0 + direction * self.phase * N as f64 / (PI * 2.0);
            let span_leds = COMET_SPAN_RAD * N as f64 / (PI * 2.0);

            for (i, led) in frame.iter_mut().enumerate() {
                let index = i as f64;
                let distance_behind = if direction > 0.0 {
                    (head - index).rem_euclid(N as f64)
                } else {
                    (index - head).rem_euclid(N as f64)
                };

                let intensity = if distance_behind <= span_leds {
                    let opacity = 1.0 - distance_behind / span_leds;
                    COMET_MIN_INTENSITY
                        + (1.0 - COMET_MIN_INTENSITY) * opacity * opacity
                } else {
                    TRACK_INTENSITY
                };

                *led = self.color * intensity * scaling_factor;
            }
        }

        AnimationState::Running
    }

    fn stop(&mut self, transition: Transition) -> eyre::Result<()> {
        match transition {
            Transition::PlayOnce | Transition::Shrink => Err(eyre!(
                "Transition {:?} not supported for PcpUploadSpinner animation",
                transition
            )),
            transition => {
                self.transition = Some(transition);
                self.transition_time = 0.0;
                Ok(())
            }
        }
    }
}
