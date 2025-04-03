use crate::engine::{
    animations::{
        alert_v2::{Alert, SquarePulseTrain},
        fake_progress_v2::FakeProgress,
    },
    Animation, AnimationState, RingFrame,
};
use orb_rgb::Argb;
use std::time::Duration;

pub const PROGRESS_BAR_FADE_OUT_DURATION: f64 = 0.3;
pub const RESULT_ANIMATION_DELAY: f64 = 0.4;

/// A composite animation that displays a fake-progress bar followed by success/failure animation.
/// It's intended to be used on the Orb's ring LEDs.
pub struct BiometricFlow<const N: usize> {
    phase: Phase<N>,
    is_success: Option<bool>,
    success_color: Argb,
    failure_color: Argb,
}

enum Phase<const N: usize> {
    FakeProgress { progress: FakeProgress<N> },
    FakeProgressFadeout { animation: Alert<N> },
    WaitingForResult,
    Result { animation: Alert<N> },
}

impl<const N: usize> BiometricFlow<N> {
    pub fn new(
        progress_color: Argb,
        progress_timeout: Duration,
        min_fast_forward_duration: Duration,
        max_fast_forward_duration: Duration,
        success_color: Argb,
        failure_color: Argb,
    ) -> Self {
        let progress = FakeProgress::<N>::new(
            progress_color,
            progress_timeout,
            min_fast_forward_duration,
            max_fast_forward_duration,
        );
        Self {
            phase: Phase::FakeProgress { progress },
            is_success: None,
            success_color,
            failure_color,
        }
    }

    /// Issues a fast-forward command to the progress bar.
    pub fn progress_fast_forward(&mut self) {
        if let Phase::FakeProgress { progress } = &mut self.phase {
            progress.set_completed();
        }
    }

    /// Sets the success state.
    pub fn set_success(&mut self, is_success: bool) {
        self.is_success = Some(is_success);
    }

    /// Returns the completion time of the progress bar.
    /// Used for the synchronization of other animations and sounds.
    pub fn get_progress_completion_time(&self) -> Duration {
        match &self.phase {
            Phase::FakeProgress { progress } => progress.get_completion_time(),
            _ => Duration::from_secs(0),
        }
    }

    /// Halts the progress bar, if it is active.
    pub fn halt_progress(&mut self) {
        if let Phase::FakeProgress { progress } = &mut self.phase {
            progress.halt();
        }
    }

    /// Resumes the progress bar, if it is active.
    pub fn resume_progress(&mut self) {
        if let Phase::FakeProgress { progress } = &mut self.phase {
            progress.resume();
        }
    }

    fn success_animation(&self, delay: f64) -> Alert<N> {
        Alert::<N>::new(
            self.success_color,
            SquarePulseTrain::from(vec![
                (0.0, 0.1),
                (0.5, 0.1),
                (1.0, 0.1),
                (1.1, 3.5),
            ]),
        )
        .unwrap()
        .with_delay(delay)
    }

    fn failure_animation(&self, delay: f64) -> Alert<N> {
        Alert::<N>::new(
            self.failure_color,
            SquarePulseTrain::from(vec![
                (0.0, 0.1),
                (0.5, 0.1),
                (1.0, 0.1),
                (1.1, 3.5),
            ]),
        )
        .unwrap()
        .with_delay(delay)
    }

    fn fake_progress_fadeout_animation(color: Argb) -> Alert<N> {
        Alert::<N>::new(
            color,
            SquarePulseTrain::from(vec![
                (0.0, 0.0),
                (0.0, PROGRESS_BAR_FADE_OUT_DURATION),
            ]),
        )
        .unwrap()
    }
}

impl<const N: usize> Animation for BiometricFlow<N> {
    type Frame = RingFrame<N>;
    fn animate(
        &mut self,
        frame: &mut Self::Frame,
        dt: f64,
        idle: bool,
    ) -> AnimationState {
        match &mut self.phase {
            Phase::FakeProgress { progress } => {
                if progress.animate(frame, dt, idle) == AnimationState::Finished {
                    self.phase = Phase::FakeProgressFadeout {
                        animation: Self::fake_progress_fadeout_animation(
                            progress.get_color(),
                        ),
                    }
                }
                AnimationState::Running
            }
            Phase::FakeProgressFadeout { animation } => {
                if animation.animate(frame, dt, idle) == AnimationState::Finished {
                    if let Some(is_success) = self.is_success {
                        self.phase = Phase::Result {
                            animation: if is_success {
                                self.success_animation(RESULT_ANIMATION_DELAY)
                            } else {
                                self.failure_animation(RESULT_ANIMATION_DELAY)
                            },
                        }
                    } else {
                        tracing::warn!(
                            "Biometric flow progress fadeout without result"
                        );
                        self.phase = Phase::WaitingForResult;
                    }
                }
                AnimationState::Running
            }
            Phase::WaitingForResult => {
                if let Some(is_success) = self.is_success {
                    self.phase = Phase::Result {
                        animation: if is_success {
                            self.success_animation(
                                PROGRESS_BAR_FADE_OUT_DURATION + RESULT_ANIMATION_DELAY,
                            )
                        } else {
                            self.failure_animation(
                                PROGRESS_BAR_FADE_OUT_DURATION + RESULT_ANIMATION_DELAY,
                            )
                        },
                    }
                }
                AnimationState::Running
            }
            Phase::Result { animation } => animation.animate(frame, dt, idle),
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn stop(&mut self, _transition: crate::engine::Transition) -> eyre::Result<()> {
        Ok(())
    }
}
