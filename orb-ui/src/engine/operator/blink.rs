use super::Animation;
use crate::engine::rgb::Rgb;
use crate::engine::{AnimationState, OperatorFrame};
use std::any::Any;

/// Blink with all LEDs.
#[derive(Default)]
pub struct Blink {
    phase: Option<f64>,
    color: Rgb,
    pattern: Vec<f64>,
}

impl Blink {
    /// Start a new blink sequence.
    pub fn trigger(&mut self, color: Rgb, pattern: Vec<f64>) {
        self.phase = Some(0.0);
        self.color = color;
        self.pattern = pattern;
    }
}

impl Animation for Blink {
    type Frame = OperatorFrame;

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn animate(
        &mut self,
        frame: &mut OperatorFrame,
        dt: f64,
        idle: bool,
    ) -> AnimationState {
        if let Some(phase) = &mut self.phase {
            *phase += dt;
            if !idle {
                let mut time_acc = 0.0;
                for (i, &time) in self.pattern.iter().enumerate() {
                    time_acc += time;
                    if *phase < time_acc {
                        let color = if i % 2 == 0 { self.color } else { Rgb::OFF };
                        for led in frame {
                            *led = color;
                        }
                        break;
                    }
                }
            }
        }
        AnimationState::Running
    }
}
