use serde::{Deserialize, Serialize};
use std::ops;
use std::ops::Add;

/// RGB LED color.
#[derive(Eq, PartialEq, Copy, Clone, Default, Debug, Serialize, Deserialize)]
pub struct Argb(
    pub Option<u8>, /* optional, dimming value, used on Diamond Orbs */
    pub u8,
    pub u8,
    pub u8,
);

impl Argb {
    pub fn lerp(self, other: Self, t: f64) -> Self {
        let t = t.clamp(0.0, 1.0);
        self * (1.0 - t) + other * t
    }
}
impl ops::Mul<f64> for Argb {
    type Output = Self;

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn mul(self, rhs: f64) -> Self::Output {
        // at low brightness (low rhs)
        // prefer to turn LED off if the resulting color has only 1 component
        // with an initial color that has more than 1 component
        if ((self.1 != 0) as u8) + ((self.2 != 0) as u8) + ((self.3 != 0) as u8) > 1 {
            let res = Argb(
                self.0,
                ((f64::from(self.1) * rhs) as u8).clamp(0, u8::MAX),
                ((f64::from(self.2) * rhs) as u8).clamp(0, u8::MAX),
                ((f64::from(self.3) * rhs) as u8).clamp(0, u8::MAX),
            );
            // if result color has only 1 component, prefer to turn LED off
            if ((res.1 != 0) as u8) + ((res.2 != 0) as u8) + ((res.3 != 0) as u8) == 1 {
                Argb(self.0, 0, 0, 0)
            } else {
                res
            }
        } else {
            Argb(
                self.0,
                ((f64::from(self.1) * rhs) as u8).clamp(0, u8::MAX),
                ((f64::from(self.2) * rhs) as u8).clamp(0, u8::MAX),
                ((f64::from(self.3) * rhs) as u8).clamp(0, u8::MAX),
            )
        }
    }
}

impl ops::MulAssign<f64> for Argb {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn mul_assign(&mut self, rhs: f64) {
        *self = *self * rhs;
    }
}

impl Add for Argb {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Argb(
            self.0,
            self.1.saturating_add(rhs.1),
            self.2.saturating_add(rhs.2),
            self.3.saturating_add(rhs.3),
        )
    }
}

#[allow(missing_docs)]
impl Argb {
    pub const DIMMING_MAX_VALUE: u8 = 31;
    pub const DEFAULT_DIMMING_VALUE: Option<u8> = Some(21);
    pub const OFF: Argb = Argb(Some(0), 0, 0, 0);
    pub const OPERATOR_DEV: Argb = { Argb(Some(Self::DIMMING_MAX_VALUE), 0, 20, 0) };

    pub const PEARL_WAVE_MIN_COLOR_INTENSITY: Argb = Argb(None, 4, 4, 4);

    pub const PEARL_OPERATOR_AMBER: Argb = Argb(None, 20, 16, 0);
    pub const PEARL_OPERATOR_DEFAULT: Argb = { Argb(None, 20, 20, 20) };
    pub const PEARL_OPERATOR_VERSIONS_DEPRECATED: Argb = Argb(None, 128, 128, 0);
    pub const PEARL_OPERATOR_VERSIONS_OUTDATED: Argb = Argb(None, 255, 0, 0);
    pub const PEARL_USER_AMBER: Argb = Argb(None, 23, 13, 0);
    pub const PEARL_USER_QR_SCAN: Argb = Argb(None, 24, 24, 24);
    pub const PEARL_USER_RED: Argb = Argb(None, 30, 2, 0);
    pub const PEARL_USER_SIGNUP: Argb = Argb(None, 31, 31, 31);
    pub const PEARL_USER_FLASH: Argb = Argb(None, 255, 255, 255);

    /// ***** Self-serve colors *****
    /// We intentionally don't include blue in most of the color scheme
    /// because a sine wave with a low blue component doesn't look good:
    /// whiter once wave is over, but darker during the wave.
    ///
    /// Outer-ring color during operator QR scans
    pub const PEARL_RING_OPERATOR_QR_SCAN: Argb = Argb(None, 20, 6, 0);
    /// Outer-ring color during wifi QR scans
    pub const PEARL_RING_WIFI_QR_SCAN: Argb = Argb(None, 0, 5, 20);
    pub const PEARL_RING_WIFI_QR_SCAN_SPINNER: Argb = Argb(None, 15, 15, 15);
    /// Outer-ring color during user QR scans
    pub const PEARL_RING_USER_QR_SCAN: Argb = Argb(None, 27, 27, 27);
    pub const PEARL_RING_USER_QR_SCAN_SPINNER: Argb = Argb(None, 28, 25, 10);
    /// Shroud color to invite user to scan / reposition in front of the orb
    pub const PEARL_CENTER_SUMMON_USER_AMBER: Argb = Argb(None, 30, 30, 30);
    /// Shroud color during user scan/capture (in progress)
    pub const PEARL_CENTER_USER_CAPTURE: Argb = Argb(None, 30, 20, 0);
    /// Outer-ring color during user scan/capture (in progress)
    pub const PEARL_RING_USER_CAPTURE: Argb = Argb(None, 30, 30, 30);
    /// Outer-ring color when capture was successful
    pub const PEARL_RING_CAPTURE_SUCCESS: Argb = Argb(None, 27, 27, 27);
    /// Shroud color when capture was successful
    pub const PEARL_CENTER_CAPTURE_SUCCESS: Argb = Argb(None, 0, 81, 0);
    /// Error color for outer ring
    pub const PEARL_RING_ERROR_SALMON: Argb = Argb(None, 24, 4, 0);

    /// ***** Self-serve colors *****
    pub const DIAMOND_OPERATOR_AMBER: Argb =
        Argb(Some(Self::DIMMING_MAX_VALUE), 20, 16, 0);
    // To help quickly distinguish dev vs prod software,
    // the default operator LED color is white for prod, yellow for dev
    pub const DIAMOND_OPERATOR_DEFAULT: Argb =
        { Argb(Some(Self::DIMMING_MAX_VALUE), 20, 25, 20) };
    pub const DIAMOND_OPERATOR_VERSIONS_DEPRECATED: Argb =
        Argb(Some(Self::DIMMING_MAX_VALUE), 128, 128, 0);
    pub const DIAMOND_OPERATOR_VERSIONS_OUTDATED: Argb =
        Argb(Some(Self::DIMMING_MAX_VALUE), 255, 0, 0);
    // this color below is yellow
    // green and blue are switched for operator LEDs
    pub const DIAMOND_OPERATOR_WIFI_MODULE_BAD: Argb = Argb(Some(10), 204, 0, 85);

    /// Outer-ring color during operator QR scans
    pub const DIAMOND_RING_OPERATOR_QR_SCAN: Argb = Argb(Some(10), 77, 14, 0);
    pub const DIAMOND_RING_OPERATOR_QR_SCAN_SPINNER: Argb = Argb(Some(20), 100, 88, 70);
    /// Outer-ring color during user QR scans
    pub const DIAMOND_RING_USER_QR_SCAN: Argb = Argb(Some(10), 60, 80, 60);
    /// Shroud color to invite user to scan / reposition in front of the orb and capture
    pub const DIAMOND_CENTER_SUMMON_USER_AMBER: Argb = Argb(Some(3), 95, 40, 3);
    /// Outer-ring color during user scan/capture (in progress)
    pub const DIAMOND_RING_USER_CAPTURE: Argb = Argb(Some(10), 120, 100, 4);
    pub const DIAMOND_CONE_AMBER: Argb = Argb(Some(Self::DIMMING_MAX_VALUE), 25, 18, 1);
    /// Error color for outer ring
    pub const DIAMOND_RING_ERROR_SALMON: Argb = Argb(Some(8), 127, 20, 0);
    /// Error color for inner ring (shroud)
    pub const DIAMOND_CENTER_ERROR_SALMON: Argb = Argb(Some(6), 255, 20, 0);

    /// QR Phase colors - diamond
    pub const DIAMOND_CENTER_WIFI_QR_SCAN: Argb = Self::DIAMOND_CENTER_USER_QR_SCAN;
    pub const DIAMOND_CENTER_OPERATOR_QR_SCAN: Argb = Self::DIAMOND_CENTER_USER_QR_SCAN;
    pub const DIAMOND_CENTER_USER_QR_SCAN: Argb = Argb(Some(6), 80, 30, 2);
    pub const DIAMOND_CENTER_USER_QR_SCAN_COMPLETED: Argb = Argb(Some(6), 230, 80, 3);
    pub const DIAMOND_CENTER_USER_QR_SCAN_SUCCESS: Argb =
        Self::DIAMOND_CENTER_USER_QR_SCAN_COMPLETED;
    pub const DIAMOND_CENTER_USER_QR_SCAN_SUCCESS_BREATHING_LOW: Argb =
        Argb(Some(6), 95, 31, 2);

    /// Biometric Capture - diamond
    pub const DIAMOND_RING_BIOMETRIC_CAPTURE_PROGRESS: Argb = Argb(Some(6), 248, 86, 4);
    pub const DIAMOND_CENTER_BIOMETRIC_CAPTURE_PROGRESS: Argb =
        Self::DIAMOND_CENTER_USER_QR_SCAN_COMPLETED;
    pub const DIAMOND_RING_BIOMETRIC_CAPTURE_SUCCESS: Argb =
        Self::DIAMOND_RING_BIOMETRIC_CAPTURE_PROGRESS;

    pub const FULL_RED: Argb = Argb(None, 255, 0, 0);
    pub const FULL_GREEN: Argb = Argb(None, 0, 255, 0);
    pub const FULL_BLUE: Argb = Argb(None, 0, 0, 255);
    pub const FULL_WHITE: Argb = Argb(None, 255, 255, 255);
    pub const FULL_BLACK: Argb = Argb(None, 0, 0, 0);

    pub fn is_off(&self) -> bool {
        self.0 == Some(0) || (self.1 == 0 && self.2 == 0 && self.3 == 0)
    }
}
