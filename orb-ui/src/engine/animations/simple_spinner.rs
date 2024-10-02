use crate::engine::animations::Static;
use crate::engine::{
    Animation, AnimationState, RingFrame, Transition, TransitionStatus,
};
use eyre::eyre;
use orb_rgb::Argb;
use std::any::Any;
use std::f64::consts::PI;

const SPIN_SPEED_SECONDS_PER_TURN: f64 = 16.0;

/// Animated spinner.
#[derive(Clone, Debug)]
pub struct SimpleSpinner<const N: usize> {
    // radians
    phase: f64,
    color: Argb,
    background: Argb,
    // rad/sec
    speed: f64,
    transition: Option<Transition>,
    transition_time: f64,
    transition_background: Option<Argb>,
}

impl<const N: usize> SimpleSpinner<N> {
    /// Creates a new [`SimpleSpinner`] with one arc.
    #[must_use]
    pub fn new(color: Argb, background: Option<Argb>) -> Self {
        Self {
            speed: 2.0 * PI / SPIN_SPEED_SECONDS_PER_TURN,
            phase: PI / 2.0, // start animation at 12 o'clock
            color,
            background: background.unwrap_or(Argb::OFF),
            transition: None,
            transition_time: 0.0,
            transition_background: None,
        }
    }

    /// Set the speed of the spinner in radians per second.
    pub fn speed(self, speed: f64) -> Self {
        Self { speed, ..self }
    }

    /// Get the current phase of the spinner.
    /// The phase is the current position of the spinner in radians.
    pub fn phase(&self) -> f64 {
        self.phase
    }

    pub fn fade_in(self, duration: f64) -> Self {
        Self {
            transition: Some(Transition::FadeIn(duration)),
            transition_time: 0.0,
            ..self
        }
    }

    pub fn background(&self) -> Argb {
        self.background
    }
}

impl<const N: usize> Animation for SimpleSpinner<N> {
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
        if let Some(Transition::ForceStop) = self.transition {
            return AnimationState::Finished;
        }

        if let Some(Transition::StartDelay(duration)) = self.transition {
            self.transition_time += dt;
            if self.transition_time >= duration {
                self.transition = None;
            }
            return AnimationState::Running;
        }

        self.phase = (self.phase + dt * self.speed) % (2.0 * PI);
        // `N - led-index` with `led-index` increasing in {0,N}, because the LED strip goes anti-clockwise
        // `N as f64 * 3.0 / 4.0` because the first LED is at 6 o'clock (3PI/2)
        let progress = (N as f64 - (self.phase * N as f64 / (2.0 * PI))
            + N as f64 * 3.0 / 4.0)
            % N as f64;
        let led_index = progress as usize;
        let head_tail_scale = progress - led_index as f64;

        if !idle {
            let mut scaling_factor = match self.transition {
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
                        self.transition_background = None;
                    }
                    (self.transition_time * PI / 2.0 / duration).sin()
                }
                _ => 1.0,
            };

            // average between background and transition_background
            // keep intensity of colors in case of a background transition, by resetting factor
            let background =
                if let Some(transition_background) = self.transition_background {
                    let b = transition_background.lerp(self.background, scaling_factor);
                    scaling_factor = 1.0;
                    b
                } else {
                    self.background
                };

            for (i, led) in frame.iter_mut().enumerate() {
                if i == led_index {
                    let c = Argb(
                        self.color.0,
                        (self.color.1 as i32
                            + ((background.1 as i32 - self.color.1 as i32) as f64
                                * (head_tail_scale))
                                as i32) as u8,
                        (self.color.2 as i32
                            + ((background.2 as i32 - self.color.2 as i32) as f64
                                * (head_tail_scale))
                                as i32) as u8,
                        (self.color.3 as i32
                            + ((background.3 as i32 - self.color.3 as i32) as f64
                                * (head_tail_scale))
                                as i32) as u8,
                    );
                    *led = c * scaling_factor;
                } else if i == (led_index + 1) % N || i == (led_index + 2) % N {
                    *led = self.color * scaling_factor;
                } else if i == (led_index + 3) % N {
                    let c = Argb(
                        self.color.0,
                        (background.1 as i32
                            + ((self.color.1 as i32 - background.1 as i32) as f64
                                * head_tail_scale) as i32)
                            as u8,
                        (background.2 as i32
                            + ((self.color.2 as i32 - background.2 as i32) as f64
                                * head_tail_scale) as i32)
                            as u8,
                        (background.3 as i32
                            + ((self.color.3 as i32 - background.3 as i32) as f64
                                * head_tail_scale) as i32)
                            as u8,
                    );
                    *led = c * scaling_factor;
                } else {
                    *led = background * scaling_factor;
                }
            }
        }

        AnimationState::Running
    }

    fn transition_from(&mut self, superseded: &dyn Any) -> TransitionStatus {
        if let Some(simple_spinner) = superseded.downcast_ref::<SimpleSpinner<N>>() {
            self.phase = simple_spinner.phase();
            self.transition_background = Some(simple_spinner.background);
            self.transition_time = 0.0;
            TransitionStatus::Smooth
        } else if let Some(static_animation) = superseded.downcast_ref::<Static<N>>() {
            self.phase = PI / 2.0; // start animation at 12 o'clock
            self.transition_background = Some(static_animation.color());
            self.transition_time = 0.0;
            TransitionStatus::Smooth
        } else {
            TransitionStatus::Sharp
        }
    }

    fn stop(&mut self, transition: Transition) -> eyre::Result<()> {
        match transition {
            Transition::Shrink | Transition::PlayOnce => {
                return Err(eyre!(
                    "Transition {:?} not supported for SimpleSpinner animation",
                    transition
                ));
            }
            t => {
                self.transition_background = None;
                self.transition_time = 0.0;
                self.transition = Some(t);
            }
        }

        Ok(())
    }
}
