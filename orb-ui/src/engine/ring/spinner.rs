use super::{render_lines, Animation, Progress};
use crate::engine::rgb::Rgb;
use crate::engine::{AnimationState, RingFrame};
use std::{any::Any, f64::consts::PI, ops::Range};

/// Maximum number of arcs.
pub const MAX_ARC_COUNT: usize = 4;

const ARC_MIN: f64 = PI / 180.0 * 20.0; // 20 degrees
const ARC_GAP: f64 = PI / 180.0 * 35.0; // 35 degrees

/// Animated spinner.
#[derive(Clone)]
pub struct Spinner<const N: usize> {
    pub(crate) shape: Shape<N>,
    speed: f64,
}

/// State to render the animation frame.
#[derive(Clone)]
pub struct Shape<const N: usize> {
    phase: f64,
    arc_min: f64,
    arc_max: f64,
    arc_count: usize,
    rotation_linear_term: f64,
    rotation_cosine_term: f64,
    transition: Transition,
    color: Rgb,
}

#[derive(Copy, Clone)]
enum Transition {
    Shrink,
    None,
}

impl<const N: usize> Spinner<N> {
    /// Creates a new [`Spinner`] with one arc.
    #[allow(dead_code)]
    #[must_use]
    pub fn single(color: Rgb) -> Self {
        Self {
            speed: PI * 2.0 / 16.0, // 16 seconds per turn
            shape: Shape {
                phase: 0.0,
                arc_min: 0.0,
                arc_max: PI * 2.0 - ARC_GAP,
                arc_count: 1,
                rotation_linear_term: 5.0,
                rotation_cosine_term: 1.3,
                transition: Transition::None,
                color,
            },
        }
    }

    /// Creates a new [`Spinner`] with three arcs.
    #[allow(dead_code)]
    #[must_use]
    pub fn triple(color: Rgb) -> Self {
        Self {
            speed: PI * 2.0 / 8.0, // 8 seconds per turn
            shape: Shape {
                phase: 0.0,
                arc_min: 0.0,
                arc_max: PI * 2.0 / 3.0 - ARC_GAP,
                arc_count: 3,
                rotation_linear_term: 1.0,
                rotation_cosine_term: 0.4,
                transition: Transition::None,
                color,
            },
        }
    }
}

impl<const N: usize> Animation for Spinner<N> {
    type Frame = RingFrame<N>;

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    #[allow(clippy::cast_precision_loss, clippy::float_cmp)]
    fn animate(
        &mut self,
        frame: &mut RingFrame<N>,
        dt: f64,
        idle: bool,
    ) -> AnimationState {
        if !idle {
            self.shape.render(frame);
        }
        self.shape.phase = (self.shape.phase + dt * self.speed) % (PI * 2.0);
        self.shape.arc_min = (self.shape.arc_min + dt * ARC_MIN * 2.0).min(ARC_MIN);
        match self.shape.transition {
            Transition::Shrink => {
                let limit = PI * 2.0 / self.shape.arc_count as f64 - ARC_GAP;
                self.shape.arc_max =
                    (self.shape.arc_max - dt * ARC_GAP / 1.0).max(limit);
                if self.shape.arc_max == limit {
                    self.shape.transition = Transition::None;
                }
                AnimationState::Running
            }
            Transition::None => AnimationState::Running,
        }
    }

    #[allow(clippy::cast_precision_loss)]
    fn transition_from(&mut self, superseded: &dyn Any) {
        if superseded.is::<Progress<N>>() {
            self.shape.transition = Transition::Shrink;
            self.shape.arc_max = PI * 2.0 / self.shape.arc_count as f64;
            self.shape.phase = PI / 2.0;
        }
    }
}

impl<const N: usize> Shape<N> {
    #[allow(clippy::cast_precision_loss)]
    pub fn render(&self, frame: &mut RingFrame<N>) {
        let start = self.phase * self.rotation_linear_term
            + (self.phase * 2.0).cos() * self.rotation_cosine_term;
        let mut arc = (1.0 - (self.phase * 2.0).cos()) * PI / self.arc_count as f64;
        arc = self.arc_min
            + arc * (self.arc_max - self.arc_min) / (PI * 2.0 / self.arc_count as f64);
        let mut ranges: [Range<f64>; MAX_ARC_COUNT] =
            [0.0..0.0, 0.0..0.0, 0.0..0.0, 0.0..0.0];
        for i in 0..self.arc_count {
            let mut start = start + PI * 2.0 / self.arc_count as f64 * i as f64;
            let end = (start + arc) % (PI * 2.0);
            start %= PI * 2.0;
            if start <= end {
                ranges[i] = start..end;
            } else {
                ranges[i] = 0.0..end;
                ranges[3] = start..PI * 2.0;
            }
        }
        render_lines(frame, Rgb::OFF, self.color, &ranges);
    }
}
