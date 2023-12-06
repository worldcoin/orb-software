use super::Animation;
use crate::engine::rgb::Rgb;
use crate::engine::{AnimationState, CenterFrame};
use std::any::Any;

/// Blink following a pattern describing the durations for each
/// edge. The first edge can be set with LEDs on or off.
pub struct Alert<const N: usize> {
    /// Currently rendered color.
    current_color: Rgb,
    target_color: Rgb,
    /// pattern contains the consecutive edges duration
    /// edge starts high/on if `start_on` is `true`, off otherwise
    /// example: vec![0.0, 0.3, 0.2, 0.3]
    ///          0.0 to 0.3 start_on, 0.3 to 0.5 !start_on, 0.5 to 0.8 start_on
    ///          ends up with the `start_on` edge
    pattern: Vec<f64>,
    /// time in animation
    phase: f64,
    /// first edge from pattern\[0\] to pattern\[1\] has LEDs on
    start_on: bool,
}

impl<const N: usize> Alert<N> {
    /// Creates a new [`Alert`].
    #[must_use]
    pub fn new(color: Rgb, pattern: Vec<f64>, start_on: bool) -> Self {
        Self {
            target_color: color,
            current_color: if start_on { color } else { Rgb::OFF },
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

        // sum up each edge duration
        for (i, &time) in self.pattern.iter().enumerate() {
            time_acc += time;
            // make sure we use the color associated with the local animation time
            if self.phase < time_acc {
                // first edge depends on `start_on`
                let mod_res = usize::from(self.start_on);
                self.current_color = if i % 2 == mod_res {
                    self.target_color
                } else {
                    Rgb::OFF
                };
                break;
            }
        }
        if !idle {
            for led in frame {
                *led = self.current_color;
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
