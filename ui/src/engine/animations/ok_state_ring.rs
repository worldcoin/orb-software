use crate::engine::{Animation, AnimationState, Transition};
use orb_rgb::Argb;
use std::{any::Any, f64::consts::PI};

/// Number of stacked levels per ring half (level 0 = bottom seam, `HALF` = top).
const HALF: usize = 18;
/// Number of discrete tetris steps. Fewer = larger, blockier segments.
const N_STEPS: usize = 7;
/// Seconds for the tracer to complete one full sweep from seam to top.
const TRACER_PERIOD: f64 = 1.2;
/// Visual width of the tracer streak in level units.
const TRACER_SIZE: f64 = 1.5;

/// OK-state outer ring animation: a tetris fill of `N_STEPS` discrete segments,
/// stacking from the bottom seam of the ring upward toward the top. Each new
/// segment fades in gradually as progress moves through it, rather than snapping
/// on. A small tracer sweeps continuously from the seam toward the top; it is only
/// visible in the unfilled region — when it enters the fill area it disappears
/// under the fill color.
///
/// The fill is driven externally via [`OkStateRing::set_progress`] (0..1).
pub struct OkStateRing<const N: usize> {
    start_color: Argb,
    end_color: Argb,
    /// Fill amount in 0..1.
    progress: f64,
    /// Accumulated time driving the tracer sweep, independent of progress.
    tracer_time: f64,
}

impl<const N: usize> OkStateRing<N> {
    pub fn new(start_color: Argb, end_color: Argb) -> Self {
        Self { start_color, end_color, progress: 0.0, tracer_time: 0.0 }
    }

    /// Sets the fill amount (0..1).
    pub fn set_progress(&mut self, progress: f64) {
        self.progress = progress.clamp(0.0, 1.0);
    }
}

impl<const N: usize> Animation for OkStateRing<N> {
    type Frame = [Argb; N];

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn animate(&mut self, frame: &mut [Argb; N], dt: f64, idle: bool) -> AnimationState {
        self.tracer_time += dt;

        // Ease-out: fast start, decelerates toward completion.
        let e = 1.0 - (1.0 - self.progress).powi(2);
        let color = self.start_color.lerp(self.end_color, e);
        let level_rad = PI / HALF as f64;
        let levels_per_step = HALF as f64 / N_STEPS as f64;

        // Map eased progress onto the N_STEPS scale.
        // current_step: which step is currently fading in (0 = bottom seam, N_STEPS-1 = top).
        // step_frac: how far into that step (0 = just started fading, 1 = fully locked).
        let step_float = e * N_STEPS as f64;
        let current_step = (step_float.floor() as usize).min(N_STEPS - 1);
        let step_frac = if step_float >= N_STEPS as f64 { 1.0 } else { step_float.fract() };

        // Tracer: sweeps from seam (level 0) to just past the top (HALF + TRACER_SIZE)
        // so its tail fully exits before the phase resets.
        let tracer_phase = (self.tracer_time / TRACER_PERIOD) % 1.0;
        let tracer_pos = tracer_phase * (HALF as f64 + TRACER_SIZE);

        if !idle {
            let one_led_rad = PI * 2.0 / N as f64;
            for (i, led) in frame.iter_mut().rev().enumerate() {
                let angle = i as f64 * one_led_rad;
                // Height from the seam, mirrored on both halves.
                let height = if angle <= PI { angle } else { PI * 2.0 - angle };
                // Continuous level position (0 = seam, HALF = top apex).
                let pos = height / level_rad;
                // Which step does this LED belong to? (0 = seam step, N_STEPS-1 = top step)
                let step_of_pos = (pos.max(0.0) / levels_per_step) as usize;
                let step_of_pos = step_of_pos.min(N_STEPS - 1);

                *led = if step_of_pos < current_step {
                    // Fully locked step: solid fill color; hides the tracer.
                    color
                } else {
                    // Fading step or unfilled: composite the fade background with the tracer.
                    // fade = 0 for unfilled steps, step_frac for the currently-filling step.
                    let fade = if step_of_pos == current_step { step_frac } else { 0.0 };

                    // Tracer comet: antialiased 1-level leading edge, TRACER_SIZE trail.
                    let d = tracer_pos - pos;
                    let tracer_b = if d < 0.0 {
                        (1.0 + d).clamp(0.0, 1.0)
                    } else {
                        1.0 - (d / TRACER_SIZE).clamp(0.0, 1.0)
                    };

                    // Take the brighter of the two so the tracer is visible sweeping
                    // through a dim fading step, and the fade persists between sweeps.
                    Argb::OFF.lerp(color, fade.max(tracer_b))
                };
            }
        }

        AnimationState::Running
    }

    fn stop(&mut self, _transition: Transition) -> eyre::Result<()> {
        Ok(())
    }
}
