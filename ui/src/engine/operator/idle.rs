//! Controls operator/button LEDs states when Orb is idle.
//!
//! On Pearl: the 5 operator LEDs are showing:
//!  - the battery status (1st LED)
//!  - the WLAN connection status (2nd LED)
//!  - the internet connection status (3rd LED)
//!
//! On Diamond: single button LED is showing:
//!  1. booting state: pulsating white (handled in boot animation, before orb-ui takes over)
//!  2. nominal state: default color (white for normal mode, colored for api mode)
//!  3. specific events (overrides other states):
//!    - battery low (critical) (red, blinking)
//!    - WLAN connection errors (amber, blinking)

use super::{compute_smooth_blink_color_multiplier, Animation};
use crate::engine::{AnimationState, OperatorFrame, OrbType};
use orb_rgb::Argb;
use std::any::Any;

const CRITICAL_BATTERY_THRESHOLD: u32 = 11;
const LOW_BATTERY_THRESHOLD: u32 = 26;

#[derive(Default, Debug)]
enum BatteryState {
    #[default]
    Discharging,
    Low,
    CriticalLow,
    Charging,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
enum Internet {
    #[default]
    Uninit,
    Good,
    Slow,
    Lost,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
enum Wlan {
    #[default]
    Uninit,
    Good,
    Slow,
    Lost,
    InitFailure,
}

/// Connection indicator.
#[derive(Default, Debug)]
pub struct Idle {
    phase: f64,
    orb_type: OrbType,
    battery: BatteryState,
    battery_percentage: u32,
    battery_is_charging: bool,
    internet: Internet,
    wlan: Wlan,
    internet_phase: f64,
    wlan_phase: f64,
    color: Argb,
}

impl Idle {
    pub fn new(orb_type: OrbType) -> Self {
        let color = match orb_type {
            OrbType::Pearl => Argb::PEARL_OPERATOR_DEFAULT,
            OrbType::Diamond => Argb::DIAMOND_OPERATOR_DEFAULT,
        };
        Self {
            orb_type,
            color,
            battery_percentage: 100,
            ..Default::default()
        }
    }

    pub fn battery_capacity(&mut self, percentage: u32) {
        self.battery_percentage = percentage;
        self.battery = if self.battery_is_charging {
            BatteryState::Charging
        } else if percentage < CRITICAL_BATTERY_THRESHOLD {
            BatteryState::CriticalLow
        } else if percentage < LOW_BATTERY_THRESHOLD {
            BatteryState::Low
        } else {
            BatteryState::Discharging
        };
    }

    pub fn battery_charging(&mut self, charging: bool) {
        self.battery_is_charging = charging;
        self.battery_capacity(self.battery_percentage);
    }

    /// Sets good internet indication.
    pub fn good_internet(&mut self) {
        self.internet = Internet::Good;
    }

    /// Sets slow internet indication.
    pub fn slow_internet(&mut self) {
        self.internet = Internet::Slow;
    }

    /// Sets no internet indication.
    pub fn no_internet(&mut self) {
        // We can't lose a connection if it has never been established.
        if self.internet != Internet::Uninit {
            self.internet = Internet::Lost;
        }
    }

    /// Sets good wlan indication.
    pub fn good_wlan(&mut self) {
        self.wlan = Wlan::Good;
    }

    /// Sets slow wlan indication.
    pub fn slow_wlan(&mut self) {
        self.wlan = Wlan::Slow;
    }

    /// Sets no wlan indication.
    pub fn no_wlan(&mut self) {
        // We can't lose a connection if it has never been established.
        if self.wlan != Wlan::Uninit {
            self.wlan = Wlan::Lost;
        }
    }

    pub fn wlan_init_failure(&mut self) {
        self.wlan = Wlan::InitFailure;
    }

    pub fn api_mode(&mut self, api_mode: bool) {
        if api_mode {
            self.color = Argb::OPERATOR_DEV;
        } else {
            match self.orb_type {
                OrbType::Pearl => {
                    self.color = Argb::PEARL_OPERATOR_DEFAULT;
                }
                OrbType::Diamond => {
                    self.color = Argb::DIAMOND_OPERATOR_DEFAULT;
                }
            }
        }
    }
}

impl Animation for Idle {
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
        // on wifi card initialization failure, the operator led stays colored in
        // DIAMOND_OPERATOR_WIFI_MODULE_BAD, cannot be overridden by other states
        if let (OrbType::Diamond, Wlan::InitFailure) = (&self.orb_type, self.wlan) {
            for f in frame {
                *f = Argb::DIAMOND_OPERATOR_WIFI_MODULE_BAD;
            }

            return AnimationState::Running;
        }

        let color_default = self.color;
        let color_amber = match self.orb_type {
            OrbType::Pearl => Argb::PEARL_OPERATOR_AMBER,
            OrbType::Diamond => Argb::DIAMOND_OPERATOR_AMBER,
        };
        let color_red = match self.orb_type {
            OrbType::Pearl => Argb::PEARL_OPERATOR_RED,
            OrbType::Diamond => Argb::DIAMOND_OPERATOR_RED,
        };

        let mut color = color_default;
        let mut diamond_mul = 1.0;
        let wlan_blink = matches!(self.wlan, Wlan::Lost | Wlan::Slow);
        let wlan_color = match self.wlan {
            Wlan::Good | Wlan::Slow => color_default,
            Wlan::Lost => color_amber,
            _ => Argb::OFF,
        };

        let mut internet_color = Argb::OFF;
        let mut internet_blink = false;
        if matches!(self.wlan, Wlan::Slow | Wlan::Good) {
            internet_color = match self.internet {
                Internet::Uninit => Argb::OFF,
                Internet::Good => color_default,
                Internet::Slow | Internet::Lost => color_amber,
            };

            internet_blink = matches!(self.internet, Internet::Lost | Internet::Slow);
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

        let (color_battery, blink) = match self.battery {
            BatteryState::Discharging => (color_default, false),
            BatteryState::Low => (color_default, true),
            BatteryState::CriticalLow => {
                if matches!(self.orb_type, OrbType::Pearl) {
                    (color_amber, false)
                } else {
                    (color_red, true)
                }
            }
            BatteryState::Charging => (color_default, false),
        };

        /* on diamond:
         * - internet connection issues should be shown over nominal state
         * - but critical battery overrides them
         */
        if matches!(self.orb_type, OrbType::Diamond) {
            if color != color_battery {
                color = color_battery;
                diamond_mul =
                    compute_smooth_blink_color_multiplier(&mut self.phase, dt);
            } else if wlan_blink || internet_blink {
                color = color_amber;
                diamond_mul =
                    compute_smooth_blink_color_multiplier(&mut self.phase, dt);
            }
        }

        let battery_m = if blink {
            compute_smooth_blink_color_multiplier(&mut self.phase, dt)
        } else {
            1.0
        };

        if !idle {
            if matches!(self.orb_type, OrbType::Pearl) {
                frame[4] = color_battery * battery_m;
                frame[3] = wlan_color * wlan_m;
                frame[2] = internet_color * internet_m;
                frame[1] = color_default;
                frame[0] = color_default;
            } else if matches!(self.orb_type, OrbType::Diamond) {
                // one single led
                frame[4] = color * diamond_mul;
            }
        }
        AnimationState::Running
    }
}
