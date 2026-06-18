use crate::engine::{Animation, AnimationState, Transition};
use orb_rgb::Argb;
use std::{any::Any, f64::consts::PI};

/// Number of stacked levels per ring half (level 0 = bottom seam, `HALF` = top).
const HALF: usize = 9;
/// Per-step rise duration of a falling block, before time-scaling (seconds).
const STEP_DUR: f64 = 0.060;
/// Pause after a block locks into place, before time-scaling (seconds).
const LOCK_PAUSE: f64 = 0.110;
/// Gap between successive blocks, before time-scaling (seconds).
const GAP: f64 = 0.050;
/// Total wall-clock duration of the stacking fill (seconds).
const FILL_DURATION: f64 = 14.0;
/// Brightness of a not-yet-lit section relative to a lit one.
const BASE_INTENSITY: f64 = 0.10;

struct Move {
    level: usize,
    t0: f64,
    t1: f64,
}

/// OK-state outer ring animation: a "tetris" stacking fill where blocks rise
/// from the bottom seam and lock in from the top down, mirrored on both halves
/// of the ring. The fill color eases from `start_color` to `end_color` as the
/// ring completes.
///
/// Mirrors the stacking (`frame`) phase of the HTML `startOkState` animation.
pub struct OkStateRing<const N: usize> {
    start_color: Argb,
    end_color: Argb,
    moves: Vec<Move>,
    lock_times: [f64; HALF + 1],
    elapsed: f64,
    transition: Option<Transition>,
    transition_time: f64,
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
        // Scale the raw timeline so the whole fill lasts exactly FILL_DURATION.
        let factor = FILL_DURATION / t;
        for m in moves.iter_mut() {
            m.t0 *= factor;
            m.t1 *= factor;
        }
        for lt in lock_times.iter_mut() {
            *lt *= factor;
        }

        Self {
            start_color,
            end_color,
            moves,
            lock_times,
            elapsed: 0.0,
            transition: None,
            transition_time: 0.0,
        }
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
        let scaling_factor = match self.transition {
            Some(Transition::ForceStop) => return AnimationState::Finished,
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
                }
                (self.transition_time * PI / 2.0 / duration).sin()
            }
            _ => 1.0,
        };

        self.elapsed += dt;
        let e = self.elapsed;

        // Warm white while filling, easing to the success color as the ring completes.
        let color_t = (e / FILL_DURATION).clamp(0.0, 1.0);
        let color = self.start_color.lerp(self.end_color, color_t);

        let moving = self
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
                let lit = moving == Some(level) || e >= self.lock_times[level];
                let intensity = if lit { 1.0 } else { BASE_INTENSITY };
                *led = color * (intensity * scaling_factor);
            }
        }

        if e >= FILL_DURATION {
            AnimationState::Finished
        } else {
            AnimationState::Running
        }
    }

    fn stop(&mut self, transition: Transition) -> eyre::Result<()> {
        self.transition = Some(transition);
        self.transition_time = 0.0;

        Ok(())
    }
}
