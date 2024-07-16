use super::Animation;
use crate::engine::{AnimationState, CenterFrame};
use orb_rgb::Argb;
use std::any::Any;
use std::f64::consts::PI;

/// Blink following a pattern describing the durations for each
/// edge. The first edge can be set with LEDs on or off.
pub struct Alert<const N: usize> {
    /// Currently rendered color.
    current_solid_color: Argb,
    target_color: Argb,
    /// pattern contains the consecutive edges duration
    /// edge starts high/on if `start_on` is `true`, off otherwise
    /// example: vec![0.0, 0.3, 0.2, 0.3]
    ///          0.0 to 0.3 start_on, 0.3 to 0.5 !start_on, 0.5 to 0.8 start_on
    ///          ends up with the `start_on` edge
    pattern: Vec<f64>,
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
    start_on: bool,
}

impl<const N: usize> Alert<N> {
    /// Creates a new [`Alert`].
    #[must_use]
    pub fn new(
        color: Argb,
        pattern: Vec<f64>,
        smooth_transitions: Option<Vec<f64>>,
        start_on: bool,
    ) -> Self {
        Self {
            target_color: color,
            current_solid_color: if start_on { color } else { Argb::OFF },
            smooth_transitions,
            pattern,
            phase: 0.0,
            start_on,
        }
    }
}

impl<const N: usize> Animation for Alert<N> {
    type Frame = CenterFrame<N>;

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn animate(
        &mut self,
        frame: &mut CenterFrame<N>,
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
                let mod_res = usize::from(self.start_on);
                self.current_solid_color = if i % 2 == (1 - mod_res) {
                    self.target_color
                } else {
                    Argb::OFF
                };
                color = self.current_solid_color;
                break;
            } else if smooth != 0.0
                && self.phase < time_acc + smooth + self.pattern[i + 1]
                && self.phase >= time_acc + smooth
            {
                // transition between edges
                let period = self.pattern[i + 1] - smooth;
                let t = self.phase - time_acc - smooth;
                let intensity = if self.current_solid_color == Argb::OFF {
                    // starts at intensity 0
                    1.0 - (t * (PI / 2.0) / period).cos()
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
