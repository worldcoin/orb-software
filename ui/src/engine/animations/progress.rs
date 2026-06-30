use crate::engine::animations::{render_lines, LIGHT_BLEEDING_OFFSET_RAD};
use crate::engine::{
    Animation, AnimationState, RingFrame, Transition, DIAMOND_RING_LED_COUNT,
};
use eyre::eyre;
use orb_rgb::Argb;
use std::{any::Any, f64::consts::PI, ops::Range};

const RC: f64 = 0.5;
const PROGRESS_REACHED_THRESHOLD: f64 = 0.01;
const PULSE_SPEED: f64 = PI * 2.0 / 4.0; // 4 seconds per wave
const PULSE_ANGLE_RAD: f64 = PI / 180.0 * 7.0; // 7º angle width
const PROGRESS_PREVIEW_PULSE_SPEED: f64 = PI * 2.0 / 8.0; // 8 seconds per wave
const PROGRESS_PREVIEW_MIN_BRIGHTNESS: f64 = 0.7;
const PROGRESS_PREVIEW_LED_COUNT: f64 = 3.0;

/// Single `color` progress ring growing clockwise from the top
/// with a `progress` value from 0.0 to 1.0
/// When `progress` is reached by the animation, the animation will
/// pulse so that it's never static
pub struct Progress<const N: usize> {
    color: Argb,
    background_color: Argb,
    /// from 0.0 to 1.0
    pub progress: f64,
    /// once `progress` reached, maintain progress ring to set `progress` during `progress_duration`
    progress_duration: Option<f64>,
    pulse_angle: f64,
    transition: Option<Transition>,
    transition_time: f64,
    pub(crate) shape: Shape<N>,
    paused: bool,
    progress_preview: bool,
}

#[derive(Clone)]
pub struct Shape<const N: usize> {
    progress: f64,
    phase: f64,
    preview_phase: f64,
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
            background_color: Argb::OFF,
            progress: initial_progress,
            progress_duration,
            pulse_angle: PULSE_ANGLE_RAD,
            transition: None,
            transition_time: 0.0,
            shape: Shape {
                progress: 0.0,
                phase: 0.0,
                preview_phase: 0.0,
                pulse_angle: PULSE_ANGLE_RAD,
            },
            paused: false,
            progress_preview: false,
        }
    }

    pub fn pause(&mut self) {
        self.paused = true;
    }

    pub fn resume(&mut self) {
        self.paused = false;
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

    pub fn with_background(mut self, color: Argb) -> Self {
        self.background_color = color;
        self
    }

    pub fn with_progress_preview(mut self) -> Self {
        self.progress_preview = true;
        self
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
            if self.progress_preview {
                self.shape.render_with_progress_preview(
                    frame,
                    self.color * scaling_factor,
                    self.background_color * scaling_factor,
                    self.progress,
                );
            } else {
                self.shape.render(
                    frame,
                    self.color * scaling_factor,
                    self.background_color * scaling_factor,
                );
            }
        }

        if self.paused {
            return AnimationState::Running;
        }

        self.shape.progress = self.shape.progress
            + (dt / (RC + dt)) * (self.progress - self.shape.progress);
        self.shape.pulse_angle = self.shape.pulse_angle
            + (dt / (RC + dt)) * (self.pulse_angle - self.shape.pulse_angle);

        if self.progress_preview {
            self.shape.preview_phase = (self.shape.preview_phase
                + dt * PROGRESS_PREVIEW_PULSE_SPEED)
                % (PI * 2.0);
        }

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
    pub fn render(&self, frame: &mut RingFrame<N>, foreground: Argb, background: Argb) {
        let mut angle_rad = 2.0 * PI * self.progress;
        // make it pulse if phase isn't null by using the sine
        angle_rad += self.phase.sin() * self.pulse_angle;
        angle_rad = angle_rad.clamp(0.0, 2.0 * PI);
        let ranges = [
            0.0..(angle_rad - PI + LIGHT_BLEEDING_OFFSET_RAD).max(0.0),
            PI + LIGHT_BLEEDING_OFFSET_RAD
                ..(PI + LIGHT_BLEEDING_OFFSET_RAD + angle_rad).min(2.0 * PI),
        ];
        render_lines(frame, background, foreground, &ranges);
    }

    #[allow(clippy::cast_precision_loss)]
    pub fn render_with_progress_preview(
        &self,
        frame: &mut RingFrame<N>,
        foreground: Argb,
        background: Argb,
        target_progress: f64,
    ) {
        let current_angle = (2.0 * PI * self.progress).clamp(0.0, 2.0 * PI);
        let target_angle = (2.0 * PI * target_progress).clamp(0.0, 2.0 * PI);
        let current_ranges = progress_ranges(current_angle);
        paint_solid_ranges(frame, Some(background), foreground, &current_ranges);

        if target_angle > current_angle {
            let preview_angle =
                target_angle.min(current_angle + progress_preview_angle::<N>());
            let preview_ranges = progress_delta_ranges(current_angle, preview_angle);
            paint_solid_ranges(
                frame,
                None,
                white_green_preview(foreground, self.preview_phase),
                &preview_ranges,
            );
        }
    }
}

fn white_green_preview(foreground: Argb, phase: f64) -> Argb {
    let brightness = PROGRESS_PREVIEW_MIN_BRIGHTNESS
        + (1.0 - PROGRESS_PREVIEW_MIN_BRIGHTNESS) * (1.0 - phase.cos()) / 2.0;

    foreground.lerp(Argb(foreground.0, 255, 255, 255), 0.35) * brightness
}

#[allow(clippy::cast_precision_loss)]
fn progress_preview_angle<const FRAME_SIZE: usize>() -> f64 {
    2.0 * PI * PROGRESS_PREVIEW_LED_COUNT / FRAME_SIZE as f64
}

fn progress_ranges(angle_rad: f64) -> [Range<f64>; 2] {
    let split_angle = PI - LIGHT_BLEEDING_OFFSET_RAD;
    [
        0.0..(angle_rad - split_angle).max(0.0),
        PI + LIGHT_BLEEDING_OFFSET_RAD
            ..(PI + LIGHT_BLEEDING_OFFSET_RAD + angle_rad).min(2.0 * PI),
    ]
}

fn progress_delta_ranges(current_angle: f64, target_angle: f64) -> Vec<Range<f64>> {
    let split_angle = PI - LIGHT_BLEEDING_OFFSET_RAD;
    let base_angle = PI + LIGHT_BLEEDING_OFFSET_RAD;

    if target_angle <= split_angle {
        vec![base_angle + current_angle..base_angle + target_angle]
    } else if current_angle < split_angle {
        vec![
            base_angle + current_angle..2.0 * PI,
            0.0..target_angle - split_angle,
        ]
    } else {
        vec![current_angle - split_angle..target_angle - split_angle]
    }
}

fn paint_solid_ranges<const FRAME_SIZE: usize>(
    frame: &mut RingFrame<FRAME_SIZE>,
    background: Option<Argb>,
    foreground: Argb,
    ranges_angle_rad: &[Range<f64>],
) {
    if FRAME_SIZE == DIAMOND_RING_LED_COUNT {
        for (i, led) in frame.iter_mut().rev().enumerate() {
            paint_solid_led::<FRAME_SIZE>(
                i,
                led,
                background,
                foreground,
                ranges_angle_rad,
            );
        }
    } else {
        for (i, led) in frame.iter_mut().enumerate() {
            paint_solid_led::<FRAME_SIZE>(
                i,
                led,
                background,
                foreground,
                ranges_angle_rad,
            );
        }
    }
}

fn paint_solid_led<const FRAME_SIZE: usize>(
    i: usize,
    led: &mut Argb,
    background: Option<Argb>,
    foreground: Argb,
    ranges_angle_rad: &[Range<f64>],
) {
    let one_led_rad = PI * 2.0 / FRAME_SIZE as f64;
    let pos = i as f64 * one_led_rad;
    if ranges_angle_rad.iter().any(|range| {
        let start_fill = pos - range.start + one_led_rad;
        let end_fill = range.end - pos;
        start_fill > 0.0 && end_fill > 0.0
    }) {
        *led = foreground;
    } else if let Some(background) = background {
        *led = background;
    }
}
