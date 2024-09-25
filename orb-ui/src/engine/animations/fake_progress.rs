use crate::engine::animations::render_lines;
use crate::engine::{Animation, AnimationState, RingFrame};
use orb_rgb::Argb;
use std::{any::Any, f64::consts::PI};

/// Progress growing from the center of the left and the right halves.
pub struct FakeProgress<const N: usize> {
    color: Argb,
    pub(crate) shape: Shape<N>,
}

#[derive(Clone)]
pub struct Shape<const N: usize> {
    duration: f64,
    phase: f64,
}

impl<const N: usize> FakeProgress<N> {
    /// Creates a new [`FakeProgress`].
    #[expect(dead_code)]
    #[must_use]
    pub fn new(duration: f64, color: Argb) -> Self {
        Self {
            color,
            shape: Shape {
                duration,
                phase: 0.0,
            },
        }
    }
}

impl<const N: usize> Animation for FakeProgress<N> {
    type Frame = RingFrame<N>;

    fn name(&self) -> &'static str {
        "FakeProgress"
    }

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
            self.shape.render(frame, self.color);
        }
        self.shape.phase += dt;
        if self.shape.phase < self.shape.duration {
            AnimationState::Running
        } else {
            AnimationState::Finished
        }
    }
}

impl<const N: usize> Shape<N> {
    #[allow(clippy::cast_precision_loss)]
    pub fn render(&self, frame: &mut RingFrame<N>, color: Argb) {
        let progress = self.phase / self.duration;
        let angle = PI * progress;
        let ranges = [PI - angle..PI, PI..PI + angle];
        render_lines(frame, Argb::OFF, color, &ranges);
    }
}
