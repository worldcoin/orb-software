//! Animations for the center LEDs.

mod alert;
#[path = "static.rs"]
mod static_;
mod wave;

pub use self::{alert::Alert, static_::Static, wave::Wave};

use super::Animation;
