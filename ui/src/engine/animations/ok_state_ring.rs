use crate::engine::{Animation, AnimationState, Transition};
use orb_rgb::Argb;
use std::{any::Any, f64::consts::PI};

/// Number of stacked levels per ring half (level 0 = bottom seam, `HALF` = top).
const HALF: usize = 18;
/// Time per level for each block's rise, controls overall animation speed.
const STEP_DUR: f64 = 0.060;
/// Length of the rising comet's trailing glow, in level units. A solid lit core
/// of this length (like the SimpleSpinner's wide arc) keeps the motion smooth
/// instead of a single LED pulsing as it splits between two levels.
const TRAIL: f64 = 4.0;

/// One block's rising journey: sweeps from level 0 to `target` over [t0, t1].
struct Block {
    target: usize,
    t0: f64,
    t1: f64,
}

/// OK-state outer ring animation: a "tetris" stacking fill where blocks rise
/// from the bottom seam and lock in from the top down, mirrored on both halves
/// of the ring. The fill is driven externally via [`OkStateRing::set_progress`]
/// (0..1), so its timing matches whatever progress source feeds it.
pub struct OkStateRing<const N: usize> {
    start_color: Argb,
    end_color: Argb,
    blocks: Vec<Block>,
    lock_times: [f64; HALF + 1],
    /// Fill amount in 0..1, mapped onto the normalized stacking schedule.
    progress: f64,
}

impl<const N: usize> OkStateRing<N> {
    pub fn new(start_color: Argb, end_color: Argb) -> Self {
        let mut blocks = Vec::new();
        let mut lock_times = [0.0; HALF + 1];
        let mut t = 0.0;
        // Each block's journey takes (target + 1) * STEP_DUR so the rising
        // speed stays constant. Blocks are back-to-back with no gap, keeping
        // the moving indicator continuously visible.
        for target in (0..=HALF).rev() {
            let dur = (target + 1) as f64 * STEP_DUR;
            blocks.push(Block { target, t0: t, t1: t + dur });
            t += dur;
            lock_times[target] = t;
        }
        for b in blocks.iter_mut() {
            b.t0 /= t;
            b.t1 /= t;
        }
        for lt in lock_times.iter_mut() {
            *lt /= t;
        }

        Self { start_color, end_color, blocks, lock_times, progress: 0.0 }
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

    fn animate(&mut self, frame: &mut [Argb; N], _dt: f64, idle: bool) -> AnimationState {
        // Ease-out the overall fill: fast from the start through the middle,
        // then decelerating toward completion (slope 2 at 0, 0 at 1).
        let e = 1.0 - (1.0 - self.progress).powi(2);
        let color = self.start_color.lerp(self.end_color, e);
        let level_rad = PI / HALF as f64;

        // Continuous head position (0..target) at constant speed (linear), like
        // the SimpleSpinner which advances its phase at a fixed rate. No easing,
        // so the rise never flicks fast then crawls — it glides evenly.
        let head: Option<f64> = self
            .blocks
            .iter()
            .find(|b| e >= b.t0 && e < b.t1)
            .map(|b| {
                let frac = ((e - b.t0) / (b.t1 - b.t0)).clamp(0.0, 1.0);
                frac * b.target as f64
            });

        if !idle {
            let one_led_rad = PI * 2.0 / N as f64;
            for (i, led) in frame.iter_mut().rev().enumerate() {
                let angle = i as f64 * one_led_rad;
                // Height up the ring from the bottom seam, mirrored on both halves.
                let height = if angle <= PI { angle } else { PI * 2.0 - angle };
                // Continuous level position of this LED (not rounded), so the
                // comet's edges antialias smoothly across adjacent LEDs.
                let pos = height / level_rad;
                let level = (pos.round() as usize).min(HALF);

                *led = if e >= self.lock_times[level] {
                    color
                } else if let Some(head) = head {
                    // Comet brightness as a function of distance below the head:
                    // a one-level antialiased leading edge, fading over `TRAIL`
                    // behind it. The solid trail keeps a lit core at all times,
                    // so motion reads as a gliding streak with no pulsing.
                    let d = head - pos;
                    let brightness = if d < 0.0 {
                        (1.0 + d).clamp(0.0, 1.0)
                    } else {
                        1.0 - (d / TRAIL).clamp(0.0, 1.0)
                    };
                    Argb::OFF.lerp(color, brightness)
                } else {
                    Argb::OFF
                };
            }
        }

        AnimationState::Running
    }

    fn stop(&mut self, _transition: Transition) -> eyre::Result<()> {
        Ok(())
    }
}
