use crate::engine::animations::{SimpleSpinner, Wave};
use crate::engine::{Animation, Transition};
use crate::engine::{AnimationState, TransitionStatus};
use eyre::eyre;
use orb_rgb::Argb;
use std::any::Any;
use std::f64::consts::PI;

/// Static color.
pub struct Static<const N: usize> {
    /// Color applied to all LEDs.
    color: Argb,
    duration: Option<f64>,
    transition: Option<Transition>,
    transition_time: f64,
    transition_background: Option<Argb>,
}

impl<const N: usize> Static<N> {
    /// Creates a new [`Static`].
    #[must_use]
    pub fn new(color: Argb, duration: Option<f64>) -> Self {
        Self {
            color,
            duration,
            transition: None,
            transition_time: 0.0,
            transition_background: None,
        }
    }

    pub fn fade_in(self, duration: f64) -> Self {
        Self {
            transition: Some(Transition::FadeIn(duration)),
            transition_time: 0.0,
            ..self
        }
    }

    /// Get the current color.
    pub fn color(&self) -> Argb {
        self.color
    }
}

impl<const N: usize> Animation for Static<N> {
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
        let mut scaling_factor = match self.transition {
            Some(Transition::ForceStop) => return AnimationState::Finished,
            Some(Transition::StartDelay(duration)) => {
                self.transition_time += dt;
                if self.transition_time >= duration {
                    self.transition = None;
                }
                return AnimationState::Running;
            }
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

        if !idle {
            // average between background and transition_background
            // keep intensity of colors in case of a background transition, by resetting factor
            let color = if let Some(transition_background) = self.transition_background
            {
                let b = transition_background.lerp(self.color, scaling_factor);
                scaling_factor = 1.0;
                b
            } else {
                self.color
            };

            for led in frame {
                *led = color * scaling_factor;
            }
        }

        if let Some(max_time) = &mut self.duration {
            *max_time -= dt;
            if *max_time <= 0.0 {
                return AnimationState::Finished;
            }
        }

        AnimationState::Running
    }

    fn transition_from(&mut self, superseded: &dyn Any) -> TransitionStatus {
        if let Some(simple_spinner) = superseded.downcast_ref::<SimpleSpinner<N>>() {
            self.transition_time = 0.0;
            self.transition_background = Some(simple_spinner.background());
            TransitionStatus::Smooth
        } else if let Some(wave) = superseded.downcast_ref::<Wave<N>>() {
            self.transition_time = 0.0;
            self.transition_background = Some(wave.color());
            TransitionStatus::Smooth
        } else {
            TransitionStatus::Sharp
        }
    }

    fn stop(&mut self, transition: Transition) -> eyre::Result<()> {
        match transition {
            Transition::PlayOnce | Transition::Shrink => {
                return Err(eyre!(
                    "Transition {:?} not supported for static animation",
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
