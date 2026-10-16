//! Circular center LED sweeps around a dark interval for ambient light sampling.
//! Each sweep lasts half a second. Start after the static QR background's fade-in;
//! its color must match and remain unchanged for a seamless handoff. Sampling
//! requests must not overlap, since a new request restarts the off sweep.

use crate::engine::{Animation, AnimationState, CenterFrame};
use orb_rgb::Argb;
use std::{any::Any, time::Duration};

const SWEEP_DURATION: f64 = 0.25;

enum Phase {
    SweepOff,
    Dark,
    SweepOn,
}

pub struct AmbientLightSample<const N: usize> {
    color: Argb,
    dark_duration: Duration,
    phase: Phase,
    elapsed: f64,
}

impl<const N: usize> AmbientLightSample<N> {
    pub fn new(color: Argb, dark_duration: Duration) -> Self {
        Self {
            color,
            dark_duration,
            phase: Phase::SweepOff,
            elapsed: 0.0,
        }
    }
}

impl<const N: usize> Animation for AmbientLightSample<N> {
    type Frame = CenterFrame<N>;

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn animate(
        &mut self,
        frame: &mut Self::Frame,
        dt: f64,
        idle: bool,
    ) -> AnimationState {
        self.elapsed += dt;
        let progress = (self.elapsed / SWEEP_DURATION).min(1.0) * N as f64;

        if !idle {
            for (index, led) in frame.iter_mut().enumerate() {
                let swept = (progress - index as f64).clamp(0.0, 1.0);
                let brightness = match self.phase {
                    Phase::SweepOff => 1.0 - swept,
                    Phase::Dark => 0.0,
                    Phase::SweepOn => swept,
                };
                *led = if brightness == 0.0 {
                    Argb::OFF
                } else {
                    self.color * brightness
                };
            }
        }

        match self.phase {
            Phase::SweepOff if self.elapsed >= SWEEP_DURATION => {
                self.phase = Phase::Dark;
                // Start the hold only after rendering a completely dark frame.
                self.elapsed = 0.0;
            }
            Phase::Dark if self.elapsed >= self.dark_duration.as_secs_f64() => {
                self.phase = Phase::SweepOn;
                self.elapsed = 0.0;
            }
            Phase::SweepOn if self.elapsed >= SWEEP_DURATION => {
                return AnimationState::Finished;
            }
            _ => {}
        }
        AnimationState::Running
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{
        animations::Static, AnimationsStack, LEVEL_BACKGROUND, LEVEL_FOREGROUND,
    };

    const COLOR: Argb = Argb::DIAMOND_CENTER_USER_QR_SCAN;

    #[test]
    fn sweeps_off_holds_dark_and_sweeps_on() {
        let mut animation = AmbientLightSample::<4>::new(COLOR, Duration::from_secs(1));
        let mut frame = [COLOR; 4];

        animation.animate(&mut frame, 0.0625, false);
        assert_eq!(frame, [COLOR * 0.5, COLOR, COLOR, COLOR]);
        animation.animate(&mut frame, 0.1875, false);
        assert_eq!(frame, [Argb::OFF, Argb::OFF, COLOR, COLOR]);
        assert_eq!(
            animation.animate(&mut frame, 0.25, false),
            AnimationState::Running
        );
        assert_eq!(frame, [Argb::OFF; 4]);

        animation.animate(&mut frame, 0.5, false);
        assert_eq!(frame, [Argb::OFF; 4]);
        animation.animate(&mut frame, 0.5, false);
        assert_eq!(frame, [Argb::OFF; 4]);

        animation.animate(&mut frame, 0.25, false);
        assert_eq!(frame, [COLOR, COLOR, Argb::OFF, Argb::OFF]);
        assert_eq!(
            animation.animate(&mut frame, 0.25, false),
            AnimationState::Finished
        );
        assert_eq!(frame, [COLOR; 4]);
    }

    #[test]
    fn delayed_off_frame_does_not_shorten_dark_interval() {
        let mut animation = AmbientLightSample::<4>::new(COLOR, Duration::from_secs(1));
        let mut frame = [COLOR; 4];

        assert_eq!(
            animation.animate(&mut frame, 10.0, false),
            AnimationState::Running
        );
        assert_eq!(frame, [Argb::OFF; 4]);
        animation.animate(&mut frame, 0.75, false);
        assert_eq!(frame, [Argb::OFF; 4]);
        animation.animate(&mut frame, 0.25, false);
        assert_eq!(frame, [Argb::OFF; 4]);
        animation.animate(&mut frame, 0.25, false);
        assert_eq!(frame, [COLOR, COLOR, Argb::OFF, Argb::OFF]);
    }

    #[test]
    fn idle_animation_does_not_write_frame() {
        let mut animation = AmbientLightSample::<4>::new(COLOR, Duration::from_secs(1));
        let mut frame = [Argb::DIAMOND_RING_ERROR_SALMON; 4];
        let expected = frame;

        for dt in [0.25, 0.25, 1.0, 0.25, 0.25] {
            animation.animate(&mut frame, dt, true);
            assert_eq!(frame, expected);
        }
    }

    #[test]
    fn zero_duration_still_sweeps_off_and_on() {
        let mut animation = AmbientLightSample::<4>::new(COLOR, Duration::ZERO);
        let mut frame = [COLOR; 4];

        assert_eq!(
            animation.animate(&mut frame, 0.5, false),
            AnimationState::Running
        );
        assert_eq!(frame, [Argb::OFF; 4]);
        animation.animate(&mut frame, 0.0, false);
        assert_eq!(
            animation.animate(&mut frame, 0.5, false),
            AnimationState::Finished
        );
        assert_eq!(frame, [COLOR; 4]);
    }

    #[test]
    fn returns_to_unchanged_background() {
        let mut stack = AnimationsStack::new();
        stack.set(LEVEL_BACKGROUND, Box::new(Static::<4>::new(COLOR, None)));
        stack.set(
            LEVEL_FOREGROUND,
            Box::new(AmbientLightSample::<4>::new(COLOR, Duration::from_secs(1))),
        );
        let mut frame = [COLOR; 4];

        stack.run(&mut frame, 0.5);
        assert_eq!(frame, [Argb::OFF; 4]);
        stack.run(&mut frame, 1.0);
        assert_eq!(frame, [Argb::OFF; 4]);
        stack.run(&mut frame, 0.5);
        assert_eq!(frame, [COLOR; 4]);
        assert!(!stack.stack.contains_key(&LEVEL_FOREGROUND));
        stack.run(&mut frame, 0.1);
        assert_eq!(frame, [COLOR; 4]);
    }
}
