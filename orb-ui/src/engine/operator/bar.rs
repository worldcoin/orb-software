use super::Animation;
use crate::engine;
use crate::engine::rgb::Argb;
use crate::engine::{AnimationState, OperatorFrame};
use std::any::Any;

/// Simple progress bar that goes from 0 to 100% or from 100 to 0%, in case `inverted`,
/// given a `duration`. The LED colors doesn't have to be overwritten
/// but can be kept from the frame passed when running the animation (see `overwrite`).
/// Finally, if one doesn't want the animation to end (typically during shutdown), it
/// can be made `endless`.
#[derive(Default)]
pub struct Bar {
    #[allow(dead_code)]
    orb_type: engine::OrbType,
    /// if inverted: from all LED in array on to all off
    inverted: bool,
    /// seconds, used to consider animation as running, the value is reset once animation is performed
    duration: f64,
    phase: f64,
    color: Argb,
    /// overwrite LED color or keep the color set in lower-priority animation
    overwrite: bool,
    /// set to endless when last animation to be displayed (ie shutting down the Orb)
    endless: bool,
}

impl Bar {
    pub fn new(orb_type: engine::OrbType) -> Self {
        Self {
            orb_type,
            ..Default::default()
        }
    }

    /// Start a new bar animation.
    pub fn trigger(
        &mut self,
        duration: f64,
        color: Argb,
        inverted: bool,
        overwrite: bool,
        endless: bool,
    ) {
        self.inverted = inverted;
        self.duration = duration;
        self.phase = 0.0;
        self.color = color;
        self.overwrite = overwrite;
        self.endless = endless;
    }
}

impl Animation for Bar {
    type Frame = OperatorFrame;

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    #[allow(
        clippy::cast_sign_loss,
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss
    )]
    fn animate(
        &mut self,
        frame: &mut OperatorFrame,
        dt: f64,
        idle: bool,
    ) -> AnimationState {
        let mut led_set_count = 0;

        // use the `duration` field to consider the animation as being performed
        // if idle or duration not set, then don't change the frame
        if !idle && self.duration > 0.0 {
            led_set_count = std::cmp::min(
                (self.phase * frame.len() as f64 / self.duration) as usize + 1_usize,
                frame.len(),
            );

            let iter: Box<dyn Iterator<Item = &mut Argb>> = if self.inverted {
                Box::new(frame.iter_mut())
            } else {
                Box::new(frame.iter_mut().rev())
            };

            for (i, led) in iter.enumerate() {
                if i < led_set_count {
                    *led = self.color;
                } else if self.overwrite {
                    *led = Argb::OFF;
                }
            }

            // stop bar animation from taking over operator LEDs when animation is over
            // by resetting duration. Next call to `trigger()` will set the new duration.
            if self.phase >= self.duration && !self.endless {
                self.duration = 0.0;
            }

            self.phase += dt;
        }

        if led_set_count != 0 && led_set_count < frame.len() {
            AnimationState::Running
        } else {
            AnimationState::Finished
        }
    }
}
