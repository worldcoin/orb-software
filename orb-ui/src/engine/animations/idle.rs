use crate::engine::Animation;
use crate::engine::{AnimationState, RingFrame};
use orb_rgb::Argb;
use std::any::Any;

/// Idle / not animated ring = all LEDs in one color
/// by default, all off
pub struct Idle<const N: usize> {
    color: Argb,
    max_time: Option<f64>,
}

impl<const N: usize> Idle<N> {
    /// Create idle ring
    #[must_use]
    pub fn new(color: Option<Argb>, max_time: Option<f64>) -> Self {
        Self {
            color: color.unwrap_or(Argb::OFF),
            max_time,
        }
    }
}

impl<const N: usize> Default for Idle<N> {
    fn default() -> Self {
        Self {
            color: Argb::OFF,
            max_time: None,
        }
    }
}

impl<const N: usize> Animation for Idle<N> {
    type Frame = RingFrame<N>;

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    #[allow(clippy::cast_precision_loss)]
    fn animate(
        &mut self,
        frame: &mut RingFrame<N>,
        dt: f64,
        idle: bool,
    ) -> AnimationState {
        if !idle {
            for led in frame {
                *led = self.color;
            }
            if let Some(max_time) = self.max_time {
                if max_time <= 0.0 {
                    return AnimationState::Finished;
                }
                self.max_time = Some(max_time - dt);
            }
        }
        AnimationState::Running
    }

    fn transition_from(&mut self, _superseded: &dyn Any) {}
}
