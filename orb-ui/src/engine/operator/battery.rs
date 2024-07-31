use super::{compute_smooth_blink_color_multiplier, Animation};
use crate::engine;
use orb_rgb::Argb;
use crate::engine::{AnimationState, OperatorFrame, OrbType};
use std::any::Any;

/// Amber blink below CRITICAL_BATTERY_THRESHOLD
/// White blink between CRITICAL_BATTERY_THRESHOLD and LOW_BATTERY_THRESHOLD
/// Solid color above

const CRITICAL_BATTERY_THRESHOLD: u32 = 11;
const LOW_BATTERY_THRESHOLD: u32 = 26;

/// Battery indicator.
pub struct Battery {
    orb_type: engine::OrbType,
    percentage: u32,
    is_charging: bool,
    phase: f64,
}

impl Battery {
    pub fn new(orb_type: engine::OrbType) -> Self {
        Self {
            orb_type,
            percentage: 100,
            is_charging: false,
            phase: 0.0,
        }
    }

    /// Sets battery capacity percentage.
    pub fn capacity(&mut self, percentage: u32) {
        self.percentage = percentage;
    }

    /// Set the charging flag.
    pub fn set_charging(&mut self, is_charging: bool) {
        self.is_charging = is_charging;
    }
}

impl Animation for Battery {
    type Frame = OperatorFrame;

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    #[allow(clippy::cast_precision_loss)]
    fn animate(
        &mut self,
        frame: &mut OperatorFrame,
        dt: f64,
        idle: bool,
    ) -> AnimationState {
        let blink = self.percentage < LOW_BATTERY_THRESHOLD;
        let color = if self.percentage < CRITICAL_BATTERY_THRESHOLD {
            match self.orb_type {
                OrbType::Pearl => Argb::PEARL_OPERATOR_AMBER,
                OrbType::Diamond => Argb::DIAMOND_OPERATOR_AMBER,
            }
        } else {
            match self.orb_type {
                OrbType::Pearl => Argb::PEARL_OPERATOR_DEFAULT,
                OrbType::Diamond => Argb::DIAMOND_OPERATOR_DEFAULT,
            }
        };
        let multiplier = if blink {
            compute_smooth_blink_color_multiplier(&mut self.phase, dt)
        } else {
            1.0
        };
        if !idle {
            frame[4] = color * multiplier;
        }
        AnimationState::Running
    }
}
