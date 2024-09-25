use crate::engine::Animation;
use crate::engine::{AnimationState, RingFrame};
use orb_rgb::Argb;
use std::{any::Any, f64::consts::PI};

const PULSE_SPEED: f64 = PI * 2.0 / 3.0; // 3 seconds per pulse

/// State of one segment.
#[expect(dead_code)]
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Segment {
    /// Segment is static off.
    Off,
    /// Segment is pulsing.
    Pulse,
    /// Segment is static on.
    Solid,
}

/// Segmented animation.
pub struct Segmented<const N: usize> {
    color: Argb,
    max_time: Option<f64>,
    pub(crate) shape: Shape<N>,
}

#[derive(Clone)]
pub struct Shape<const N: usize> {
    start_angle: f64,
    pattern: Vec<Segment>,
    phase: f64,
}

impl<const N: usize> Segmented<N> {
    /// Creates a new [`Segmented`].
    #[expect(dead_code)]
    #[must_use]
    pub fn new(
        color: Argb,
        start_angle: f64,
        pattern: Vec<Segment>,
        max_time: Option<f64>,
    ) -> Self {
        Self {
            color,
            max_time,
            shape: Shape {
                start_angle: start_angle % PI,
                pattern,
                phase: 0.0,
            },
        }
    }

    /// Returns a mutable slice of the segmented pattern.
    #[expect(dead_code)]
    pub fn pattern_mut(&mut self) -> &mut [Segment] {
        &mut self.shape.pattern
    }
}

impl<const N: usize> Animation for Segmented<N> {
    type Frame = RingFrame<N>;

    fn name(&self) -> &'static str {
        "Segmented"
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
        self.shape.phase = (self.shape.phase + dt * PULSE_SPEED) % (PI * 2.0);
        if let Some(max_time) = &mut self.max_time {
            *max_time -= dt;
            if *max_time <= 0.0 {
                return AnimationState::Finished;
            }
        }
        AnimationState::Running
    }
}

impl<const N: usize> Shape<N> {
    const LED: f64 = PI * 2.0 / N as f64;

    #[allow(clippy::cast_precision_loss, clippy::match_on_vec_items)]
    pub fn render(&self, frame: &mut RingFrame<N>, color: Argb) {
        let pulse_color = color * ((1.0 - self.phase.cos()) / 2.0);
        for (led_index, led) in frame.iter_mut().enumerate() {
            *led = match self.pattern[self.segment_index(led_index)] {
                Segment::Off => Argb::OFF,
                Segment::Pulse => pulse_color,
                Segment::Solid => color,
            };
        }
    }

    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    fn segment_index(&self, led_index: usize) -> usize {
        let led_angle =
            (PI + self.start_angle + led_index as f64 * Self::LED) % (PI * 2.0);
        (led_angle / (PI * 2.0 / self.pattern.len() as f64)) as usize
    }
}
