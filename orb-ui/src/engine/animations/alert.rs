use crate::engine::rgb::Argb;
use crate::engine::{Animation, AnimationState};
use std::any::Any;
use std::f64::consts::PI;

/// Alert animation.
/// Blink following a pattern describing the durations for each edge.
/// The first edge can be set with LEDs on or off.
/// The pattern contains the consecutive edges duration.
/// Smooth transitions can optionally be set to start a sine wave to transition between edges.

/// Pattern contains the consecutive edges duration
/// Edge starts high/on if `active_at_start` is `true`, off otherwise
/// Example: vec![0.0, 0.3, 0.2, 0.3]
///          0.0 to 0.3 `active_at_start`, 0.3 to 0.5 `!active_at_start`, 0.5 to 0.8 `active_at_start`
///          ends up with the `active_at_start` edge
type Pattern = Vec<f64>;

/// Blink following a pattern describing the durations for each
/// edge. The first edge can be set with LEDs on or off.
pub struct Alert<const N: usize> {
    /// Currently rendered color.
    current_solid_color: Argb,
    target_color: Argb,
    /// pattern contains the consecutive edges duration
    pattern: Pattern,
    /// allows to start a sine wave to transition between edges.
    /// The color will start to change from the current color to the target color by the time given
    /// between `smooth_transitions[i]` and `pattern[i+1]`.
    /// example with pattern vector above, and `smooth_transitions`:
    ///         Some(vec![0.1, 0.1, 0.1])
    ///         t=0.0 to t=0.1 start_on, t=0.1 to t=0.3 transition to !start_on, etc.
    smooth_transitions: Option<Vec<f64>>,
    /// time in animation
    phase: f64,
    /// first edge from pattern\[0\] to pattern\[1\] has LEDs on
    active_at_start: bool,
}

impl<const N: usize> Alert<N> {
    /// Creates a new [`Alert`].
    #[must_use]
    pub fn new(
        color: Argb,
        pattern: Vec<f64>,
        smooth_transitions: Option<Vec<f64>>,
        active_at_start: bool,
    ) -> Self {
        Self {
            target_color: color,
            current_solid_color: if active_at_start { color } else { Argb::OFF },
            smooth_transitions,
            pattern,
            phase: 0.0,
            active_at_start,
        }
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
        let mut time_acc = 0.0;
        let mut color = Argb::OFF;

        // sum up each edge duration
        for (i, &time) in self.pattern.iter().enumerate() {
            time_acc += time;
            // make sure we use the color associated with the local animation time
            let smooth = if let Some(s) = self.smooth_transitions.as_ref() {
                s[i]
            } else {
                0.0
            };

            if self.phase < time_acc + smooth {
                // first edge [i = 0] depends on `start_on`
                let mod_res = usize::from(self.active_at_start);
                self.current_solid_color = if i % 2 == mod_res {
                    self.target_color
                } else {
                    Argb::OFF
                };
                color = self.current_solid_color;
                break;
            } else if smooth != 0.0
                && self.phase < time_acc + self.pattern[i + 1]
                && self.phase >= time_acc + smooth
            {
                // transition between edges
                let period = self.pattern[i + 1] - smooth;
                let t = self.phase - time_acc - smooth;
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

        if self.phase < self.pattern.iter().sum::<f64>() {
            AnimationState::Running
        } else {
            AnimationState::Finished
        }
    }
}

/// Test, use the example
/// pattern: vec![0.0, 0.3, 0.2, 0.3]
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
            vec![0.0, 0.3, 0.2, 0.3],
            None,
            true,
        );
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
