use super::Animation;
use crate::engine;
use crate::engine::{AnimationState, OperatorFrame};
use orb_rgb::Argb;
use std::any::Any;

/// Blink with all LEDs.
#[derive(Default)]
pub struct Blink {
    #[allow(dead_code)]
    orb_type: engine::OrbType,
    phase: Option<f64>,
    color: Argb,
    pattern: Vec<f64>,
}

impl Blink {
    pub fn new(orb_type: engine::OrbType) -> Self {
        Self {
            orb_type,
            ..Default::default()
        }
    }

    /// Start a new blink sequence.
    pub fn trigger(&mut self, color: Argb, pattern: Vec<f64>) {
        self.phase = Some(0.0);
        self.color = color;
        self.pattern = pattern;
    }
}

impl Animation for Blink {
    type Frame = OperatorFrame;

    fn name(&self) -> &'static str {
        "Operator Blink"
    }

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
                        let color = if i % 2 == 0 { self.color } else { Argb::OFF };
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
