use super::Animation;
use crate::engine::rgb::Rgb;
use crate::engine::{AnimationState, RingFrame};
use std::any::Any;

/// Static color.
pub struct Static<const N: usize> {
    /// Currently rendered color.
    current_color: Rgb,
    max_time: Option<f64>,
    stop: bool,
}

impl<const N: usize> Static<N> {
    /// Creates a new [`Static`].
    #[must_use]
    pub fn new(color: Rgb, max_time: Option<f64>) -> Self {
        Self {
            current_color: color,
            max_time,
            stop: false,
        }
    }
}

impl<const N: usize> Animation for Static<N> {
    type Frame = RingFrame<N>;

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn animate(
        &mut self,
        frame: &mut RingFrame<N>,
        dt: f64,
        idle: bool,
    ) -> AnimationState {
        if !idle {
            for led in frame {
                *led = self.current_color;
            }
        }
        if let Some(max_time) = &mut self.max_time {
            *max_time -= dt;
            if *max_time <= 0.0 {
                return AnimationState::Finished;
            }
        }
        if self.stop {
            AnimationState::Finished
        } else {
            AnimationState::Running
        }
    }

    fn stop(&mut self) {
        self.stop = true;
    }
}
