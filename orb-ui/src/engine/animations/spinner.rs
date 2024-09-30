use crate::engine::animations::{render_lines, MilkyWay, Progress, SimpleSpinner};
use crate::engine::{
    Animation, AnimationState, RingFrame, Transition, TransitionStatus,
};
use eyre::eyre;
use orb_rgb::Argb;
use std::{any::Any, f64::consts::PI, ops::Range};

/// Maximum number of arcs.
pub const MAX_ARC_COUNT: usize = 4;

const ARC_MIN: f64 = PI / 180.0 * 20.0; // 20 degrees
const ARC_GAP: f64 = PI / 180.0 * 35.0; // 35 degrees

const SPIN_SPEED_SECONDS_PER_TURN: f64 = 25.0;

/// Animated spinner.
#[derive(Clone)]
pub struct Spinner<const N: usize> {
    pub(crate) shape: Shape<N>,
    // rad/sec
    speed: f64,
    transition_time: f64,
}

/// State to render the animation frame.
#[derive(Clone)]
pub struct Shape<const N: usize> {
    // radians
    phase: f64,
    arc_min: f64,
    arc_max: f64,
    arc_count: usize,
    rotation_linear_term: f64,
    rotation_cosine_term: f64,
    transition: Option<Transition>,
    color: Argb,
    background: Argb,
    color_scale: f64,
}

impl<const N: usize> Spinner<N> {
    /// Creates a new [`Spinner`] with one arc.
    #[must_use]
    #[expect(dead_code)]
    pub fn single(color: Argb, background: Option<Argb>) -> Self {
        Self {
            speed: PI * 2.0 / SPIN_SPEED_SECONDS_PER_TURN,
            transition_time: 0.0,
            shape: Shape {
                phase: 0.0,
                arc_min: 0.0,
                arc_max: PI * 2.0 - ARC_GAP,
                arc_count: 1,
                rotation_linear_term: 1.0,
                rotation_cosine_term: 1.3,
                transition: None,
                color,
                background: background.unwrap_or(Argb::OFF),
                color_scale: 1.0,
            },
        }
    }

    /// Creates a new [`Spinner`] with three arcs.
    #[must_use]
    pub fn triple(color: Argb, background: Option<Argb>) -> Self {
        Self {
            speed: PI * 2.0 / 8.0, // 8 seconds per turn
            transition_time: 0.0,
            shape: Shape {
                phase: 0.0,
                arc_min: 0.0,
                arc_max: PI * 2.0 / 3.0 - ARC_GAP,
                arc_count: 3,
                rotation_linear_term: 1.0,
                rotation_cosine_term: 1.0,
                transition: None,
                color,
                background: background.unwrap_or(Argb::OFF),
                color_scale: 1.0,
            },
        }
    }

    #[expect(dead_code)]
    pub fn arc_min(mut self, arc_min: f64) -> Self {
        self.shape.arc_min = arc_min;
        self
    }

    #[expect(dead_code)]
    pub fn arc_max(mut self, arc_max: f64) -> Self {
        self.shape.arc_max = arc_max;
        self
    }

    #[expect(dead_code)]
    pub fn fade_in(self, duration: f64) -> Self {
        Self {
            transition_time: 0.0,
            shape: Shape {
                transition: Some(Transition::FadeIn(duration)),
                ..self.shape
            },
            ..self
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
        self.shape.phase = (self.shape.phase + dt * self.speed) % (PI * 2.0);
        self.shape.arc_min = (self.shape.arc_min + dt * ARC_MIN * 2.0).min(ARC_MIN);
        let animation_state = match self.shape.transition {
            Some(Transition::ForceStop) => return AnimationState::Finished,
            Some(Transition::Shrink) => {
                let limit = PI * 2.0 / self.shape.arc_count as f64 - ARC_GAP;
                self.shape.arc_max =
                    (self.shape.arc_max - dt * ARC_GAP / 1.0).max(limit);
                if self.shape.arc_max == limit {
                    self.shape.transition = None;
                }
                AnimationState::Running
            }
            Some(Transition::StartDelay(delay)) => {
                self.transition_time += dt;
                if self.transition_time >= delay {
                    self.shape.transition = None;
                }
                AnimationState::Running
            }
            Some(Transition::FadeIn(duration)) => {
                self.transition_time += dt;
                if self.transition_time >= duration {
                    self.shape.transition = None;
                }
                self.shape.color_scale =
                    (self.transition_time * PI / 2.0 / duration).sin();
                AnimationState::Running
            }
            Some(Transition::FadeOut(duration)) => {
                self.transition_time += dt;
                self.shape.color_scale =
                    (self.transition_time * PI / 2.0 / duration).cos();
                if self.transition_time >= duration {
                    return AnimationState::Finished;
                }
                AnimationState::Running
            }
            _ => AnimationState::Running,
        };

        if !idle {
            self.shape.render(frame);
        }

        animation_state
    }

    #[allow(clippy::cast_precision_loss)]
    fn transition_from(&mut self, superseded: &dyn Any) -> TransitionStatus {
        if superseded.is::<Progress<N>>() {
            self.shape.transition = Some(Transition::Shrink);
            self.shape.arc_max = PI * 2.0 / self.shape.arc_count as f64;
            self.shape.phase = PI / 2.0;
            TransitionStatus::Smooth
        } else if superseded.is::<MilkyWay<N>>() {
            TransitionStatus::Smooth
        } else if let Some(simple_spinner) =
            superseded.downcast_ref::<SimpleSpinner<N>>()
        {
            self.shape.phase = simple_spinner.phase();
            TransitionStatus::Smooth
        } else {
            TransitionStatus::Sharp
        }
    }

    fn stop(&mut self, transition: Transition) -> eyre::Result<()> {
        if transition == Transition::PlayOnce {
            return Err(eyre!(
                "transition {:? } is not supported for spinner animation",
                transition
            ));
        }

        self.transition_time = 0.0;
        self.shape.transition = Some(transition);

        Ok(())
    }
}

impl<const N: usize> Shape<N> {
    #[allow(clippy::cast_precision_loss)]
    pub fn render(&self, frame: &mut RingFrame<N>) {
        let start = 2.0 * PI - self.phase;
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
        render_lines(
            frame,
            self.background * self.color_scale,
            self.color * self.color_scale,
            &ranges,
        );
    }
}
