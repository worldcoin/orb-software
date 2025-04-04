use crate::engine::animations::{render_lines, LIGHT_BLEEDING_OFFSET_RAD};
use crate::engine::{Animation, AnimationState, RingFrame, Transition};
use eyre::eyre;
use orb_rgb::Argb;
use std::{any::Any, f64::consts::PI};

const RC: f64 = 0.5;
const PROGRESS_REACHED_THRESHOLD: f64 = 0.01;
const PULSE_SPEED: f64 = PI * 2.0 / 4.0; // 4 seconds per wave
const PULSE_ANGLE_RAD: f64 = PI / 180.0 * 7.0; // 7º angle width

/// Single `color` progress ring growing clockwise from the top
/// with a `progress` value from 0.0 to 1.0
/// When `progress` is reached by the animation, the animation will
/// pulse so that it's never static
pub struct Progress<const N: usize> {
    color: Argb,
    /// from 0.0 to 1.0
    pub progress: f64,
    /// once `progress` reached, maintain progress ring to set `progress` during `progress_duration`
    progress_duration: Option<f64>,
    pulse_angle: f64,
    transition: Option<Transition>,
    transition_time: f64,
    pub(crate) shape: Shape<N>,
}

#[derive(Clone)]
pub struct Shape<const N: usize> {
    progress: f64,
    phase: f64,
    pulse_angle: f64,
}

impl<const N: usize> Progress<N> {
    /// Creates a new [`Progress`].
    /// Progress initial value can be set and will never decrease when calling
    /// [`Progress::set_progress()`]
    #[must_use]
    pub fn new(
        initial_progress: f64,
        progress_duration: Option<f64>,
        color: Argb,
    ) -> Self {
        Self {
            color,
            progress: initial_progress,
            progress_duration,
            pulse_angle: PULSE_ANGLE_RAD,
            transition: None,
            transition_time: 0.0,
            shape: Shape {
                progress: 0.0,
                phase: 0.0,
                pulse_angle: PULSE_ANGLE_RAD,
            },
        }
    }

    /// Sets the progress value for the progress ring [0.0, 1.0]
    /// `progress` can only increase.
    /// It is allowed to set a higher value that 1.0 to make the ring visually
    /// progress to 1.0 without slowing down close to 1.0
    /// Value is clamped if outside allowed range of [0.0, 2.0]
    pub fn set_progress(&mut self, progress: f64, progress_duration: Option<f64>) {
        if progress > self.progress {
            self.progress = progress.clamp(0.0, 2.0);
            self.progress_duration = progress_duration;
        }
    }

    /// Sets the target pulse angle width.
    pub fn set_pulse_angle(&mut self, pulse_angle: f64) {
        self.pulse_angle = pulse_angle;
    }

    pub fn get_color(&self) -> Argb {
        self.color
    }
}

impl<const N: usize> Animation for Progress<N> {
    type Frame = RingFrame<N>;

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
        let scaling_factor = match self.transition {
            Some(Transition::ForceStop) => return AnimationState::Finished,
            Some(Transition::StartDelay(duration)) => {
                self.transition_time += dt;
                if self.transition_time >= duration {
                    self.transition = None;
                }
                return AnimationState::Running;
            }
            Some(Transition::FadeOut(duration)) => {
                self.transition_time += dt;
                if self.transition_time >= duration {
                    return AnimationState::Finished;
                }
                (self.transition_time * PI / 2.0 / duration).cos()
            }
            Some(Transition::FadeIn(duration)) => {
                self.transition_time += dt;
                if self.transition_time >= duration {
                    self.transition = None;
                }
                (self.transition_time * PI / 2.0 / duration).sin()
            }
            _ => 1.0,
        };

        tracing::trace!("scaling: {scaling_factor}");
        if !idle {
            self.shape.render(frame, self.color * scaling_factor);
        }

        self.shape.progress = self.shape.progress
            + (dt / (RC + dt)) * (self.progress - self.shape.progress);
        self.shape.pulse_angle = self.shape.pulse_angle
            + (dt / (RC + dt)) * (self.pulse_angle - self.shape.pulse_angle);

        if let Some(progress_duration) = &mut self.progress_duration {
            // if progress is reached by the shape, we animate the progress with a pulse
            // if not, we smoothly go back to a phase of 0.0
            if (self.progress - self.shape.progress) <= PROGRESS_REACHED_THRESHOLD
                && self.progress < (1.0 - LIGHT_BLEEDING_OFFSET_RAD / (2.0 * PI))
            {
                *progress_duration -= dt;
                self.shape.phase = (self.shape.phase + dt * PULSE_SPEED) % (PI * 2.0);
            } else if self.shape.phase > 0.0 {
                self.shape.phase = (self.shape.phase - dt * PULSE_SPEED).max(0.0);
            }
            // animation is over once the shape progress is very close to the set progress and
            // `progress_duration` has been fully spent
            if *progress_duration > 0.0
                || (self.progress - self.shape.progress) > PROGRESS_REACHED_THRESHOLD
            {
                AnimationState::Running
            } else {
                AnimationState::Finished
            }
        } else {
            self.shape.phase = (self.shape.phase + dt * PULSE_SPEED) % (PI * 2.0);
            if self.progress < 1.0
                || (self.progress - self.shape.progress) > PROGRESS_REACHED_THRESHOLD
            {
                AnimationState::Running
            } else {
                AnimationState::Finished
            }
        }
    }

    fn stop(&mut self, transition: Transition) -> eyre::Result<()> {
        match transition {
            Transition::PlayOnce | Transition::Shrink => {
                return Err(eyre!(
                    "Transition {:?} not supported for static animation",
                    transition
                ));
            }
            t => {
                self.transition = Some(t);
                self.transition_time = 0.0;
            }
        }

        Ok(())
    }
}

impl<const N: usize> Shape<N> {
    #[allow(clippy::cast_precision_loss)]
    pub fn render(&self, frame: &mut RingFrame<N>, color: Argb) {
        let mut angle_rad = 2.0 * PI * self.progress;
        // make it pulse if phase isn't null by using the sine
        angle_rad += self.phase.sin() * self.pulse_angle;
        angle_rad = angle_rad.clamp(0.0, 2.0 * PI);
        let ranges = [
            0.0..(angle_rad - PI + LIGHT_BLEEDING_OFFSET_RAD).max(0.0),
            PI + LIGHT_BLEEDING_OFFSET_RAD
                ..(PI + LIGHT_BLEEDING_OFFSET_RAD + angle_rad).min(2.0 * PI),
        ];
        render_lines(frame, Argb::OFF, color, &ranges);
    }
}
