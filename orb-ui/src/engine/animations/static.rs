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

#[cfg(test)]
mod test {
    use crate::engine::animations::r#static::TRANSITION_DURATION;
    use crate::engine::animations::Static;
    use crate::engine::{Animation, AnimationState};
    use orb_rgb::Argb;

    /// test transitions between static animations
    /// from 20 to 0 brightness between the 2 solid colors
    #[test]
    fn test_transition() {
        let initial_color = Argb(Some(15), 1, 1, 1);
        let final_color = Argb(Some(0), 2, 2, 2);
        let mut static1 = Static::<1>::new(initial_color, None);
        let mut static2 = Static::<1>::new(final_color, None);

        let mut frame = [Argb::OFF];
        let dt = 0.1;

        // transition from static1 to static2
        static1.animate(&mut frame, dt, false);
        assert_eq!(frame[0], initial_color);
        static2.transition_from(&static1);

        let mut total_time = 0.0;
        while total_time < TRANSITION_DURATION / 2.0 {
            static2.animate(&mut frame, dt, false);
            total_time += dt;
        }

        assert_eq!(
            frame[0].0,
            Some(initial_color.0.unwrap() / 2),
            "brightness should have decrease by 2: {:?} < {:?}",
            frame[0].0,
            Argb::DIAMOND_USER_AMBER.0
        );
        assert_eq!(frame[0].1, final_color.1, "red");
        assert_eq!(frame[0].2, final_color.2, "green");
        assert_eq!(frame[0].3, final_color.3, "blue");

        let mut state = AnimationState::Finished;
        while total_time <= TRANSITION_DURATION {
            state = static2.animate(&mut frame, dt, false);
            total_time += dt;
            println!("t: {}, frame: {:?}", total_time, frame[0]);
        }

        assert!(static2.transition_duration_left < 0.0);
        assert_eq!(state, AnimationState::Running);
        assert_eq!(
            frame[0], final_color,
            "transition should be over: {:?}",
            frame[0]
        );
    }
}
