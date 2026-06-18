use crate::engine::{Animation, AnimationState, Transition};
use orb_rgb::Argb;
use std::{any::Any, f64::consts::PI};

/// Number of stacked levels per ring half (level 0 = bottom seam, `HALF` = top).
/// More levels => smaller, more numerous chunks.
const HALF: usize = 18;
/// Per-step rise duration of a falling block (relative; the schedule is
/// normalized to 0..1 and then driven externally by progress).
const STEP_DUR: f64 = 0.060;
/// Pause after a block locks into place (relative).
const LOCK_PAUSE: f64 = 0.110;
/// Gap between successive blocks (relative).
const GAP: f64 = 0.050;

struct Move {
    level: usize,
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
    moves: Vec<Move>,
    lock_times: [f64; HALF + 1],
    /// Fill amount in 0..1, mapped onto the normalized stacking schedule.
    progress: f64,
}

impl<const N: usize> OkStateRing<N> {
    pub fn new(start_color: Argb, end_color: Argb) -> Self {
        let mut moves = Vec::new();
        let mut lock_times = [0.0; HALF + 1];
        let mut t = 0.0;
        // Blocks land top-first: each target rises from the bottom through every
        // level below it, then locks at its resting place.
        for target in (0..=HALF).rev() {
            for level in 0..=target {
                moves.push(Move {
                    level,
                    t0: t,
                    t1: t + STEP_DUR,
                });
                t += STEP_DUR;
            }
            lock_times[target] = t;
            t += LOCK_PAUSE + GAP;
        }
        // Normalize the schedule to 0..1 so it can be driven by `progress`.
        for m in moves.iter_mut() {
            m.t0 /= t;
            m.t1 /= t;
        }
        for lt in lock_times.iter_mut() {
            *lt /= t;
        }

        Self {
            start_color,
            end_color,
            moves,
            lock_times,
            progress: 0.0,
        }
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
        let e = self.progress;
        let color = self.start_color.lerp(self.end_color, e);

        let moving_level = self
            .moves
            .iter()
            .find(|m| e >= m.t0 && e < m.t1)
            .map(|m| m.level);

        if !idle {
            let one_led_rad = PI * 2.0 / N as f64;
            let level_rad = PI / HALF as f64;
            for (i, led) in frame.iter_mut().rev().enumerate() {
                let angle = i as f64 * one_led_rad;
                // Height up the ring from the bottom seam, mirrored on both halves.
                let height = if angle <= PI { angle } else { PI * 2.0 - angle };
                let level = ((height / level_rad).round() as usize).min(HALF);
                // The rising block lights only the single nearest LED per side,
                // so the travelling chunk is smaller than a locked one.
                let is_moving = moving_level.is_some_and(|ml| {
                    (height - ml as f64 * level_rad).abs() < one_led_rad / 2.0
                });
                let lit = is_moving || e >= self.lock_times[level];
                *led = if lit { color } else { Argb::OFF };
            }
        }

        AnimationState::Running
    }

    fn stop(&mut self, _transition: Transition) -> eyre::Result<()> {
        Ok(())
    }
}
