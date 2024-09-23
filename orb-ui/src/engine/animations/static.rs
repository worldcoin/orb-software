use crate::engine::AnimationState;
use crate::engine::{Animation, Transition};
use eyre::eyre;
use orb_rgb::Argb;
use std::any::Any;
use std::f64::consts::PI;

/// Static color.
pub struct Static<const N: usize> {
    /// Currently rendered color.
    current_color: Argb,
    duration: Option<f64>,
    transition: Option<Transition>,
    transition_time: f64,
}

impl<const N: usize> Static<N> {
    /// Creates a new [`Static`].
    #[must_use]
    pub fn new(color: Argb, duration: Option<f64>) -> Self {
        Self {
            current_color: color,
            duration,
            transition: None,
            transition_time: 0.0,
        }
    }

    #[allow(dead_code)]
    pub fn fade_in(self, duration: f64) -> Self {
        Self {
            transition: Some(Transition::FadeIn(duration)),
            transition_time: 0.0,
            ..self
        }
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
        let intensity = match self.transition {
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
            for led in frame {
                *led = self.current_color * intensity;
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
