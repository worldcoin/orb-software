//! Animations for the operator LEDs.

mod bar;
mod battery;
mod blink;
mod connection;
mod pulse;
mod signup_phase;

pub use self::{
    bar::Bar, battery::Battery, blink::Blink, connection::Connection, pulse::Pulse,
    signup_phase::SignupPhase,
};
use std::f64::consts::PI;

use super::Animation;

// even with a 2-second wave, the blink is noticeable only a few hundreds milliseconds
const SMOOTH_BLINK_SOLID_PERIOD_SEC: f64 = 2.0;
const SMOOTH_BLINK_FULL_PERIOD_SEC: f64 = 4.0;

/// Operator LED blinking scheme computation
/// wave then solid using 2 different periods
/// `phase` is updated using `dt`
fn compute_smooth_blink_color_multiplier(phase: &mut f64, dt: f64) -> f64 {
    // 2 different transformations:
    // one for the wave, one for the solid state
    let mut next_ph = *phase % (PI * 2.0 + SMOOTH_BLINK_SOLID_PERIOD_SEC);
    if next_ph < PI * 2.0 {
        // during the wave
        next_ph += dt
            * (PI * 2.0
                / (SMOOTH_BLINK_FULL_PERIOD_SEC - SMOOTH_BLINK_SOLID_PERIOD_SEC/* wave period */));
    } else {
        // solid
        next_ph += dt;
    }

    *phase = next_ph;

    if *phase < 2.0 * PI {
        (phase.cos() + 1.0) / 2.0
    } else {
        1.0
    }
}
