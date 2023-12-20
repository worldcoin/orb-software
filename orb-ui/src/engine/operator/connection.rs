use super::{compute_smooth_blink_color_multiplier, Animation};
use crate::engine::rgb::Argb;
use crate::engine::{AnimationState, OperatorFrame, OrbType};
use std::any::Any;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Quality {
    #[default]
    Uninit,
    Good,
    Slow,
    Lost,
}

/// Connection indicator.
#[derive(Default)]
pub struct Connection {
    orb_type: OrbType,
    internet: Quality,
    wlan: Quality,
    internet_phase: f64,
    wlan_phase: f64,
}

impl Connection {
    pub fn new(orb_type: OrbType) -> Self {
        Self {
            orb_type,
            ..Default::default()
        }
    }

    /// Sets good internet indication.
    pub fn good_internet(&mut self) {
        self.internet = Quality::Good;
    }

    /// Sets slow internet indication.
    pub fn slow_internet(&mut self) {
        self.internet = Quality::Slow;
    }

    /// Sets no internet indication.
    pub fn no_internet(&mut self) {
        // We can't loose a connection if it has never been established.
        if self.internet != Quality::Uninit {
            self.internet = Quality::Lost;
        }
    }

    /// Sets good wlan indication.
    pub fn good_wlan(&mut self) {
        self.wlan = Quality::Good;
    }

    /// Sets slow wlan indication.
    pub fn slow_wlan(&mut self) {
        self.wlan = Quality::Slow;
    }

    /// Sets no wlan indication.
    pub fn no_wlan(&mut self) {
        // We can't loose a connection if it has never been established.
        if self.wlan != Quality::Uninit {
            self.wlan = Quality::Lost;
        }
    }
}

impl Animation for Connection {
    type Frame = OperatorFrame;

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn animate(
        &mut self,
        frame: &mut OperatorFrame,
        dt: f64,
        idle: bool,
    ) -> AnimationState {
        let color_default = match self.orb_type {
            OrbType::Pearl => Argb::PEARL_OPERATOR_DEFAULT,
            OrbType::Diamond => Argb::DIAMOND_OPERATOR_DEFAULT,
        };
        let color_amber = match self.orb_type {
            OrbType::Pearl => Argb::PEARL_OPERATOR_AMBER,
            OrbType::Diamond => Argb::DIAMOND_OPERATOR_AMBER,
        };

        let wlan_blink = matches!(self.wlan, Quality::Lost | Quality::Slow);
        let wlan_color = match self.wlan {
            Quality::Uninit => Argb::OFF,
            Quality::Good | Quality::Slow => color_default,
            Quality::Lost => color_amber,
        };

        let mut internet_color = Argb::OFF;
        let mut internet_blink = false;
        if matches!(self.wlan, Quality::Slow | Quality::Good) {
            internet_color = match self.internet {
                Quality::Uninit => Argb::OFF,
                Quality::Good => color_default,
                Quality::Slow | Quality::Lost => color_amber,
            };

            internet_blink = matches!(self.internet, Quality::Lost | Quality::Slow);
        }

        let internet_m = if internet_blink {
            compute_smooth_blink_color_multiplier(&mut self.internet_phase, dt)
        } else {
            1.0
        };
        let wlan_m = if wlan_blink {
            compute_smooth_blink_color_multiplier(&mut self.wlan_phase, dt)
        } else {
            1.0
        };

        if !idle {
            frame[3] = wlan_color * wlan_m;
            frame[2] = internet_color * internet_m;
            frame[1] = if internet_color == color_amber {
                color_default
            } else {
                internet_color
            };
            frame[0] = frame[1];
        }
        AnimationState::Running
    }
}
