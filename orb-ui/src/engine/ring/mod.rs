//! Animations for the ring LEDs.

pub use self::{
    arc_pulse::ArcPulse, idle::Idle, progress::Progress, slider::Slider,
    spinner::Spinner,
};
use super::Animation;
use crate::engine::rgb::Argb;
use crate::engine::{RingFrame, DIAMOND_RING_LED_COUNT, GAMMA};
use std::{f64::consts::PI, ops::Range};

pub mod alert;
mod arc_dash;
mod arc_pulse;
mod fake_progress;
mod idle;
mod progress;
mod segmented;
mod slider;
mod spinner;
pub mod r#static;
pub mod wave;

const LIGHT_BLEEDING_OFFSET_RAD: f64 = PI / 180.0 * 6.0; // 6° offset of the start to compensate for light bleeding.

/// Renders a set of lines with smooth ends.
#[allow(clippy::cast_precision_loss)]
pub fn render_lines<const FRAME_SIZE: usize, const RANGES_COUNT: usize>(
    frame: &mut RingFrame<FRAME_SIZE>,
    background: Argb,
    foreground: Argb,
    ranges_angle_rad: &[Range<f64>; RANGES_COUNT],
) {
    if FRAME_SIZE == DIAMOND_RING_LED_COUNT {
        'leds: for (i, led) in frame.iter_mut().rev().enumerate() {
            let one_led_rad = PI * 2.0 / FRAME_SIZE as f64;
            let pos = i as f64 * one_led_rad;
            for &Range { start, end } in ranges_angle_rad {
                let start_fill = pos - start + one_led_rad;
                if start_fill <= 0.0 {
                    continue;
                }
                let end_fill = end - pos;
                if end_fill <= 0.0 {
                    continue;
                }
                *led = foreground;
                if start_fill < one_led_rad || end_fill < one_led_rad {
                    *led *= ((start_fill.min(one_led_rad) + end_fill.min(one_led_rad)
                        - one_led_rad)
                        / one_led_rad)
                        .powf(GAMMA);
                }
                continue 'leds;
            }
            *led = background;
        }
    } else {
        'leds: for (i, led) in frame.iter_mut().enumerate() {
            let one_led_rad = PI * 2.0 / FRAME_SIZE as f64;
            let pos = i as f64 * one_led_rad;
            for &Range { start, end } in ranges_angle_rad {
                let start_fill = pos - start + one_led_rad;
                if start_fill <= 0.0 {
                    continue;
                }
                let end_fill = end - pos;
                if end_fill <= 0.0 {
                    continue;
                }
                *led = foreground;
                if start_fill < one_led_rad || end_fill < one_led_rad {
                    *led *= ((start_fill.min(one_led_rad) + end_fill.min(one_led_rad)
                        - one_led_rad)
                        / one_led_rad)
                        .powf(GAMMA);
                }
                continue 'leds;
            }
            *led = background;
        }
    }
}
