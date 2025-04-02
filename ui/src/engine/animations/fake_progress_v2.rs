use super::Progress;
use crate::engine::{Animation, AnimationState, RingFrame};
use orb_rgb::Argb;
use std::{any::Any, time::Duration};

/// Exponentially decaying progress bar.
/// The bar keeps progressing until a given timeout.
/// If all of the tasks complete before the timeout,
/// the bar smoothly progresses towards the end in `fast_forward_duration` time.
struct ProgressBar {
    progress: f64,
    rate: f64,
    completed: bool,
    min_fast_forward_duration: Duration,
    max_fast_forward_duration: Duration,
}

pub struct FakeProgress<const N: usize> {
    progress_bar: ProgressBar,
    progress_animation: Progress<N>,
    halted: bool,
}

impl<const N: usize> Animation for FakeProgress<N> {
    type Frame = RingFrame<N>;

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
    ) -> crate::engine::AnimationState {
        self.progress_bar.update(dt);
        if !self.halted {
            self.progress_animation
                .set_progress(self.progress_bar.progress, None);
        }
        match self.progress_animation.animate(frame, dt, idle) {
            AnimationState::Finished => AnimationState::Finished,
            AnimationState::Running if self.progress_animation.progress >= 1.0 => {
                AnimationState::Finished
            }
            _ => AnimationState::Running,
        }
    }
}

impl<const N: usize> FakeProgress<N> {
    pub fn new(
        color: Argb,
        timeout: Duration,
        min_fast_forward_duration: Duration,
        max_fast_forward_duration: Duration,
    ) -> Self {
        Self {
            progress_bar: ProgressBar::new(
                timeout,
                min_fast_forward_duration,
                max_fast_forward_duration,
            ),
            progress_animation: Progress::<N>::new(0.0, None, color),
            halted: false,
        }
    }

    pub fn set_completed(&mut self) -> Duration {
        self.progress_bar.set_completed()
    }

    #[expect(dead_code)]
    pub fn halt(&mut self) {
        self.halted = true;
    }

    #[expect(dead_code)]
    pub fn resume(&mut self) {
        self.halted = false;
    }
}

impl ProgressBar {
    // higher than 1.0, so that the `displayed_progress` can reach 1.0.
    const TARGET_PROGRESS: f64 = 1.1;

    pub fn new(
        timeout: Duration,
        min_fast_forward_duration: Duration,
        max_fast_forward_duration: Duration,
    ) -> Self {
        let rate = -f64::ln((Self::TARGET_PROGRESS - 1.0) / Self::TARGET_PROGRESS)
            / timeout.as_secs_f64();
        Self {
            progress: 0.0,
            rate,
            completed: false,
            min_fast_forward_duration,
            max_fast_forward_duration,
        }
    }

    pub fn update(&mut self, dt: f64) -> f64 {
        // Use exponential smoothing to move displayed_progress toward effective_target.
        self.progress +=
            (Self::TARGET_PROGRESS - self.progress) * (1.0 - (-self.rate * dt).exp());
        self.progress
    }

    #[expect(dead_code)]
    pub fn is_complete(&self) -> bool {
        self.progress >= 1.0
    }

    pub fn set_completed(&mut self) -> Duration {
        self.completed = true;
        // linear interpolation of max..min fast-forward duration based on current progress.
        let fast_forward_duration = self.max_fast_forward_duration.as_secs_f64()
            + self.progress
                * (self.min_fast_forward_duration.as_secs_f64()
                    - self.max_fast_forward_duration.as_secs_f64());
        self.rate = -f64::ln(
            (Self::TARGET_PROGRESS - 1.0) / (Self::TARGET_PROGRESS - self.progress),
        ) / fast_forward_duration;

        Duration::from_secs_f64(fast_forward_duration)
    }
}
