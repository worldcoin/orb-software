use crate::engine::{
    animations::{fake_progress_v2::FakeProgress, OkStateRing},
    Animation, AnimationState, RingFrame, Transition,
};
use orb_rgb::Argb;
use std::{f64::consts::PI, time::Duration};

/// Highest fill the fake progress bar may reach on its own. It holds here
/// (just short of full) until the fast-forward completes the ring.
const WAIT_CAP: f64 = 0.85;

/// Duration of the pre-stacking breathing pulse (one half-sine inhale → exhale).
const BREATH_DUR: f64 = 0.7;
/// Peak brightness of the pre-stacking breath, as a fraction of the start color.
const BREATH_PEAK: f64 = 0.4;

/// Composite OK-state ring animation. The "tetris" stacking fill only runs
/// while the user is in the OK state (in range and not occluded); otherwise the
/// ring is off and the fill is frozen, resuming when the OK state returns.
///
/// The fill amount is driven by an internal fake-progress bar, so its timing
/// matches a loading bar.
pub struct OkState<const N: usize> {
    phase: Phase<N>,
    in_range: bool,
    occluded: bool,
    /// Once the fill completes it stays solid (success state), ignoring the OK
    /// gating, so the ring holds white after signup rather than going off.
    completed: bool,
    /// Once signup completes and the fill is fast-forwarding, it must run to
    /// completion regardless of the OK gating, so a momentary out-of-range or
    /// occlusion at the finish line can't turn the ring off mid-animation.
    fast_forwarding: bool,
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
        /// Accumulated time inside the OK state, used to gate the pre-stacking breath.
        breath_elapsed: f64,
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
            in_range: false,
            occluded: false,
            completed: false,
            fast_forwarding: false,
            start_color,
            end_color,
            progress_color,
            progress_timeout,
            min_fast_forward_duration,
            max_fast_forward_duration,
        }
    }

    /// Whether the user is within capture range.
    pub fn set_in_range(&mut self, in_range: bool) {
        self.in_range = in_range;
    }

    /// Whether the capture is currently occluded.
    pub fn set_occluded(&mut self, occluded: bool) {
        self.occluded = occluded;
    }

    fn is_ok(&self) -> bool {
        self.in_range && !self.occluded
    }

    /// Fast-forwards the fill to completion (no-op before stacking has started).
    pub fn fast_forward(&mut self) {
        if let Phase::Stacking { progress, .. } = &mut self.phase {
            progress.set_completed();
            self.fast_forwarding = true;
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
        let ok = self.is_ok();
        let fast_forwarding = self.fast_forwarding;

        // Start the tetris fill once the OK state is first reached.
        if ok && matches!(self.phase, Phase::Waiting) {
            self.phase = Phase::Stacking {
                ring: OkStateRing::<N>::new(self.start_color, self.end_color),
                progress: FakeProgress::<N>::new(
                    self.progress_color,
                    self.progress_timeout,
                    self.min_fast_forward_duration,
                    self.max_fast_forward_duration,
                ),
                breath_elapsed: 0.0,
            };
        }

        let mut just_completed = false;
        match &mut self.phase {
            Phase::Stacking { ring, .. } if self.completed => {
                // Success state: hold the ring solid, ignoring the OK gating.
                ring.set_progress(1.0);
                ring.animate(frame, dt, idle);
            }
            Phase::Stacking { ring, progress, breath_elapsed } if ok || fast_forwarding => {
                // Advance the progress bar for timing only (idle => no render),
                // then drive the tetris fill from its displayed progress.
                let state = progress.animate(frame, dt, true);
                // The fake progress bar must never complete the ring on its own:
                // while waiting it holds just short of full, and only the
                // fast-forward (real signup success) drives it home and locks in
                // the solid success state.
                let displayed = if fast_forwarding {
                    progress.progress()
                } else {
                    progress.progress().min(WAIT_CAP)
                };

                if *breath_elapsed < BREATH_DUR && !fast_forwarding {
                    // Pre-stacking breath: pulse the full ring once (half-sine) before
                    // the tetris blocks start rising, smoothing the off→on transition.
                    *breath_elapsed += dt;
                    let t = (*breath_elapsed / BREATH_DUR).min(1.0);
                    let breath = (PI * t).sin() * BREATH_PEAK;
                    if !idle {
                        let breath_color = Argb::OFF.lerp(self.start_color, breath);
                        frame.iter_mut().for_each(|led| *led = breath_color);
                    }
                } else {
                    ring.set_progress(displayed);
                    ring.animate(frame, dt, idle);
                }

                if state == AnimationState::Finished && fast_forwarding {
                    just_completed = true;
                }
            }
            // Waiting, or not in the OK state: ring off, fill frozen.
            _ => frame.iter_mut().for_each(|led| *led = Argb::OFF),
        }
        if just_completed {
            self.completed = true;
        }

        AnimationState::Running
    }

    fn stop(&mut self, _transition: Transition) -> eyre::Result<()> {
        Ok(())
    }
}
