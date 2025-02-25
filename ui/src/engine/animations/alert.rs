//! Alert animation.
//! Blink following a pattern describing the durations for each edge.
//! The first edge can be set with LEDs on or off.
//! The `blinks` contains the consecutive edges duration.
//! Smooth transitions can optionally be set to start a sine wave to transition between edges.

use crate::engine::{Animation, AnimationState};
use orb_rgb::Argb;
use std::any::Any;
use std::f64::consts::PI;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AlertError {
    #[error("The number of smooth transitions must be equal to the number of blinks - 1")]
    MismatchSmoothTransitions,
}

/// BlinkDurations contains the consecutive edges duration
/// Starts high/on if `active_at_start` is `true`, off otherwise
/// Example: vec![0.0, 0.3, 0.2, 0.3]
///          0.0 to 0.3 `active_at_start`, 0.3 to 0.5 `!active_at_start`, 0.5 to 0.8 `active_at_start`
///          ends up with the `active_at_start` edge
pub struct BlinkDurations(Vec<f64>);

impl From<Vec<f64>> for BlinkDurations {
    fn from(value: Vec<f64>) -> Self {
        Self(value)
    }
}

/// Blink following a pattern describing the durations for each
/// edge. The first edge can be set with LEDs on or off.
pub struct Alert<const N: usize> {
    /// Currently rendered color.
    current_solid_color: Argb,
    target_color: Argb,
    /// pattern contains the consecutive edges duration
    blinks: BlinkDurations,
    /// allows to start a sine wave to transition between edges.
    /// The color will start to change from the current color to the target color by the time given
    /// between `smooth_transitions[i]` and `blinks.0[i+1]`.
    /// example with
    ///     `blinks = BlinkDurations::from(vec![0.0, 0.3, 0.2, 0.3])`
    ///     `smooth_transitions = Some(vec![0.1, 0.1, 0.1])`
    ///      t=0.0 to t=0.1 `active_at_start`, t=0.1 to t=0.3 transition to `!active_at_start`, etc.
    smooth_transitions: Option<Vec<f64>>,
    /// time in animation
    phase: f64,
    /// first edge from pattern\[0\] to pattern\[1\] has LEDs on
    active_at_start: bool,
    /// initial delay, in seconds, before starting the animation
    initial_delay: f64,
}

impl<const N: usize> Alert<N> {
    /// Creates a new [`Alert`].
    pub fn new(
        color: Argb,
        blinks: BlinkDurations,
        smooth_transitions: Option<Vec<f64>>,
        active_at_start: bool,
    ) -> Result<Self, AlertError> {
        if smooth_transitions.as_ref().is_some_and(|t| t.len() != blinks.0.len() - 1) {
                return Err(AlertError::MismatchSmoothTransitions);
        }
        Ok(Self {
            target_color: color,
            current_solid_color: if active_at_start { color } else { Argb::OFF },
            smooth_transitions,
            blinks,
            phase: 0.0,
            active_at_start,
            initial_delay: 0.0,
        })
    }

    #[expect(dead_code)]
    pub fn with_delay(mut self, delay: f64) -> Self {
        self.initial_delay = delay;
        self
    }
}

impl<const N: usize> Animation for Alert<N> {
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
        let mut duration_acc = 0.0;
        let mut color = Argb::OFF;

        // initial delay
        if self.initial_delay > 0.0 {
            self.initial_delay -= dt;
            return AnimationState::Running;
        }

        // sum up each edge duration and quit when the phase is in the current edge
        for (i, &edge_duration) in self.blinks.0.iter().enumerate() {
            duration_acc += edge_duration;
            // The color starts to change from the current color to the target color by the time given
            // between `smooth_transitions[i]` and `blink[i+1]`.
            let smooth_start_time = if let Some(s) = self.smooth_transitions.as_ref() {
                s[i]
            } else {
                0.0
            };

            if self.phase < duration_acc + smooth_start_time {
                // first edge [i = 0] depends on `active_at_start`
                let mod_res = usize::from(self.active_at_start);
                self.current_solid_color = if i % 2 == mod_res {
                    self.target_color
                } else {
                    Argb::OFF
                };
                color = self.current_solid_color;
                break;
            } else if smooth_start_time != 0.0
                && self.phase < duration_acc + self.blinks.0[i + 1]
                && self.phase >= duration_acc + smooth_start_time
            {
                // transition between edges
                let period = self.blinks.0[i + 1] - smooth_start_time;
                let t = self.phase - duration_acc - smooth_start_time;
                let intensity = if self.current_solid_color == Argb::OFF {
                    // starts at intensity 0
                    1.0 - (t / period * (PI / 2.0)).cos()
                } else {
                    // starts at intensity 1
                    (t * (PI / 2.0) / period).cos()
                };
                color = self.target_color * intensity;
                break;
            }
        }
        if !idle {
            for led in frame {
                *led = color;
            }
        }
        self.phase += dt;

        if self.phase < self.blinks.0.iter().sum::<f64>() {
            AnimationState::Running
        } else {
            AnimationState::Finished
        }
    }
}

/// Test, use the example
/// blinks: vec![0.0, 0.3, 0.2, 0.3]
/// expected: 0.0 to 0.3 `active_at_start`, 0.3 to 0.5 `!active_at_start`, 0.5 to 0.8 `active_at_start`
///          ends up with the `active_at_start` edge
#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_alert() {
        let mut frame = [Argb::OFF; 1];
        let mut alert = Alert::<1>::new(
            Argb::DIAMOND_OPERATOR_AMBER,
            BlinkDurations(vec![0.0, 0.3, 0.2, 0.3]),
            None,
            true,
        ).unwrap();
        let dt = 0.1;
        let mut time = 0.0;
        let idle = false;
        let mut state = AnimationState::Running;
        while state == AnimationState::Running {
            state = alert.animate(&mut frame, dt, idle);
            if time < 0.3 {
                assert_eq!(frame[0], Argb::DIAMOND_OPERATOR_AMBER, "time: {time}");
            } else if time < 0.5 {
                assert_eq!(frame[0], Argb::OFF, "time: {time}");
            } else if time <= 0.8 {
                assert_eq!(frame[0], Argb::DIAMOND_OPERATOR_AMBER, "time: {time}");
            } else {
                // should not end up here
                assert_eq!(frame[0], Argb::OFF, "time: {time}");
            }
            time += dt;
        }
    }
}
