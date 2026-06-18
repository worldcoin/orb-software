use crate::engine::{
    animations::{fake_progress_v2::FakeProgress, OkStateRing},
    Animation, AnimationState, RingFrame, Transition,
};
use orb_rgb::Argb;
use std::time::Duration;

/// Composite OK-state ring animation:
/// 1. `Waiting` — ring stays off until [`OkState::start_stacking`] is called
///    (i.e. the user reaches an OK position).
/// 2. `Stacking` — the "tetris" stacking fill, whose fill amount is driven by
///    an internal fake-progress bar (so its timing matches a loading bar).
pub struct OkState<const N: usize> {
    phase: Phase<N>,
    start_color: Argb,
    end_color: Argb,
    progress_color: Argb,
    progress_timeout: Duration,
    min_fast_forward_duration: Duration,
    max_fast_forward_duration: Duration,
}

enum Phase<const N: usize> {
    Waiting,
    Stacking {
        ring: OkStateRing<N>,
        progress: FakeProgress<N>,
    },
}

impl<const N: usize> OkState<N> {
    pub fn new(
        start_color: Argb,
        end_color: Argb,
        progress_color: Argb,
        progress_timeout: Duration,
        min_fast_forward_duration: Duration,
        max_fast_forward_duration: Duration,
    ) -> Self {
        Self {
            phase: Phase::Waiting,
            start_color,
            end_color,
            progress_color,
            progress_timeout,
            min_fast_forward_duration,
            max_fast_forward_duration,
        }
    }

    /// Starts the tetris stacking fill, ending the initial off/waiting phase.
    /// No-op once the sequence has already started.
    pub fn start_stacking(&mut self) {
        if matches!(self.phase, Phase::Waiting) {
            self.phase = Phase::Stacking {
                ring: OkStateRing::<N>::new(self.start_color, self.end_color),
                progress: FakeProgress::<N>::new(
                    self.progress_color,
                    self.progress_timeout,
                    self.min_fast_forward_duration,
                    self.max_fast_forward_duration,
                ),
            };
        }
    }

    /// Fast-forwards the fill to completion (no-op before stacking has started).
    pub fn fast_forward(&mut self) {
        if let Phase::Stacking { progress, .. } = &mut self.phase {
            progress.set_completed();
        }
    }

    /// Completion time of the fill, for sound synchronization.
    pub fn get_progress_completion_time(&self) -> Duration {
        match &self.phase {
            Phase::Stacking { progress, .. } => progress.get_completion_time(),
            Phase::Waiting => Duration::from_secs(0),
        }
    }
}

impl<const N: usize> Animation for OkState<N> {
    type Frame = RingFrame<N>;

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn animate(
        &mut self,
        frame: &mut Self::Frame,
        dt: f64,
        idle: bool,
    ) -> AnimationState {
        match &mut self.phase {
            Phase::Waiting => {
                frame.iter_mut().for_each(|led| *led = Argb::OFF);
                AnimationState::Running
            }
            Phase::Stacking { ring, progress } => {
                // Advance the progress bar for timing only (idle => no render),
                // then drive the tetris fill from its displayed progress.
                progress.animate(frame, dt, true);
                ring.set_progress(progress.progress());
                ring.animate(frame, dt, idle);
                AnimationState::Running
            }
        }
    }

    fn stop(&mut self, _transition: Transition) -> eyre::Result<()> {
        Ok(())
    }
}
