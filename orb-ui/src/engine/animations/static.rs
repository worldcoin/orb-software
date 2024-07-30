use crate::engine::animations::Wave;
use crate::engine::Animation;
use crate::engine::AnimationState;
use orb_rgb::Argb;
use std::any::Any;
use tracing::info;

const TRANSITION_DURATION: f64 = 1.5;

/// Static color.
pub struct Static<const N: usize> {
    target_color: Argb,
    transition_original_color: Option<Argb>,
    transition_duration_left: f64,
    max_time: Option<f64>,
    stop: bool,
}

impl<const N: usize> Static<N> {
    /// Creates a new [`Static`].
    #[must_use]
    pub fn new(color: Argb, max_time: Option<f64>) -> Self {
        Self {
            target_color: color,
            transition_original_color: None,
            transition_duration_left: 0.0,
            max_time,
            stop: false,
        }
    }
}

impl<const N: usize> Default for Static<N> {
    fn default() -> Self {
        Self {
            target_color: Argb::OFF,
            transition_original_color: None,
            transition_duration_left: 0.0,
            max_time: None,
            stop: false,
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
        // smooth transition from previous static color
        let color = if let Some(transition_original) = self.transition_original_color {
            let color = Argb::brightness_lerp(
                transition_original,
                self.target_color,
                1.0 - self.transition_duration_left / TRANSITION_DURATION,
            );

            // remove transition after duration
            self.transition_duration_left -= dt;
            if self.transition_duration_left <= 0.0 {
                self.transition_original_color = None;
            }

            color
        } else {
            self.target_color
        };

        // update frame
        if !idle {
            for led in frame {
                *led = color;
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

    fn transition_from(&mut self, superseded: &dyn Any) {
        if let Some(other) = superseded.downcast_ref::<Static<N>>() {
            self.transition_original_color = Some(other.target_color);
            self.transition_duration_left = TRANSITION_DURATION;
            info!(
                "Transitioning from Static to Static ({:?} -> {:?}).",
                other.target_color, self.target_color
            );
        }
        if let Some(other) = superseded.downcast_ref::<Wave<N>>() {
            info!("Transitioning from Wave to Static.");
            self.transition_original_color = Some(other.current());
            self.transition_duration_left = TRANSITION_DURATION;
        }
    }
}
