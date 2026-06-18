use crate::engine::{
    animations::{fake_progress_v2::FakeProgress, OkStateRing},
    Animation, AnimationState, RingFrame, Transition,
};
use orb_rgb::Argb;
use std::time::Duration;

/// Composite OK-state ring animation, played in three phases:
/// 1. `Waiting` — ring stays off until [`OkState::start_stacking`] is called
///    (i.e. the user reaches an OK position).
/// 2. `Stacking` — the "tetris" stacking fill (warm-white → success color).
/// 3. `Loading` — a visible fake-progress loading bar.
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
    Stacking { ring: OkStateRing<N> },
    Loading { progress: FakeProgress<N> },
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
            };
        }
    }

    /// Fast-forwards the loading bar to completion (no-op in other phases).
    pub fn fast_forward(&mut self) {
        if let Phase::Loading { progress } = &mut self.phase {
            progress.set_completed();
        }
    }

    /// Completion time of the loading bar, for sound synchronization.
    pub fn get_progress_completion_time(&self) -> Duration {
        match &self.phase {
            Phase::Loading { progress } => progress.get_completion_time(),
            _ => Duration::from_secs(0),
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
            Phase::Stacking { ring } => {
                if ring.animate(frame, dt, idle) == AnimationState::Finished {
                    self.phase = Phase::Loading {
                        progress: FakeProgress::<N>::new(
                            self.progress_color,
                            self.progress_timeout,
                            self.min_fast_forward_duration,
                            self.max_fast_forward_duration,
                        ),
                    };
                }
                AnimationState::Running
            }
            Phase::Loading { progress } => progress.animate(frame, dt, idle),
        }
    }

    fn stop(&mut self, _transition: Transition) -> eyre::Result<()> {
        Ok(())
    }
}
