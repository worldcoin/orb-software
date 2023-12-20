use serde::{Deserialize, Serialize};
use std::ops;

/// RGB LED color.
#[derive(Eq, PartialEq, Copy, Clone, Default, Debug, Serialize, Deserialize)]
pub struct Argb(
    pub Option<u8>, /* optional, dimming value, used on Diamond Orbs */
    pub u8,
    pub u8,
    pub u8,
);

impl ops::Mul<f64> for Argb {
    type Output = Self;

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn mul(self, rhs: f64) -> Self::Output {
        // if intensity is led by the dimming value, use it
        // otherwise, modify the color values
        if let Some(dim) = self.0 {
            Argb(
                Some(((f64::from(dim) * rhs) as u8).clamp(0, Self::DIMMING_MAX_VALUE)),
                self.1,
                self.2,
                self.3,
            )
        } else {
            Argb(
                None,
                ((f64::from(self.1) * rhs) as u8).clamp(0, 255),
                ((f64::from(self.2) * rhs) as u8).clamp(0, 255),
                ((f64::from(self.3) * rhs) as u8).clamp(0, 255),
            )
        }
    }
}

impl ops::MulAssign<f64> for Argb {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn mul_assign(&mut self, rhs: f64) {
        // if intensity is led by the dimming value, use it
        // otherwise, modify the color values
        if let Some(dim) = self.0 {
            self.0 =
                Some(((f64::from(dim) * rhs) as u8).clamp(0, Self::DIMMING_MAX_VALUE));
        } else {
            self.1 = ((f64::from(self.1) * rhs) as u8).clamp(0, 255);
            self.2 = ((f64::from(self.2) * rhs) as u8).clamp(0, 255);
            self.3 = ((f64::from(self.3) * rhs) as u8).clamp(0, 255);
        };
    }
}

#[allow(missing_docs)]
impl Argb {
    pub(crate) const DIMMING_MAX_VALUE: u8 = 31;
    pub(crate) const OFF: Argb = Argb(Some(0), 0, 0, 0);
    pub(crate) const PEARL_OPERATOR_AMBER: Argb = Argb(None, 20, 16, 0);
    // To help quickly distinguish dev vs prod software,
    // the default operator LED color is white for prod, yellow for dev
    pub(crate) const PEARL_OPERATOR_DEFAULT: Argb = {
        #[cfg(not(feature = "stage"))]
        {
            Argb(None, 20, 20, 20)
        }
        #[cfg(feature = "stage")]
        {
            Argb(None, 8, 25, 8)
        }
    };
    pub(crate) const PEARL_OPERATOR_VERSIONS_DEPRECATED: Argb = Argb(None, 128, 128, 0);
    pub(crate) const PEARL_OPERATOR_VERSIONS_OUTDATED: Argb = Argb(None, 255, 0, 0);
    pub(crate) const PEARL_USER_AMBER: Argb = Argb(None, 23, 13, 0);
    pub(crate) const PEARL_USER_QR_SCAN: Argb = Argb(None, 24, 24, 24);
    pub(crate) const PEARL_USER_RED: Argb = Argb(None, 30, 2, 0);
    pub(crate) const PEARL_USER_SIGNUP: Argb = Argb(None, 31, 31, 31);
    pub(crate) const PEARL_USER_FLASH: Argb = Argb(None, 255, 255, 255);

    pub(crate) const DIAMOND_OPERATOR_AMBER: Argb =
        Argb(Some(Self::DIMMING_MAX_VALUE), 20, 16, 0);
    // To help quickly distinguish dev vs prod software,
    // the default operator LED color is white for prod, yellow for dev
    pub(crate) const DIAMOND_OPERATOR_DEFAULT: Argb = {
        #[cfg(not(feature = "stage"))]
        {
            Argb(Some(Self::DIMMING_MAX_VALUE), 20, 25, 20)
        }
        #[cfg(feature = "stage")]
        {
            Argb(Some(Self::DIMMING_MAX_VALUE), 8, 25, 8)
        }
    };
    pub(crate) const DIAMOND_OPERATOR_VERSIONS_DEPRECATED: Argb =
        Argb(Some(Self::DIMMING_MAX_VALUE), 128, 128, 0);
    pub(crate) const DIAMOND_OPERATOR_VERSIONS_OUTDATED: Argb =
        Argb(Some(Self::DIMMING_MAX_VALUE), 255, 0, 0);
    pub(crate) const DIAMOND_USER_AMBER: Argb =
        Argb(Some(Self::DIMMING_MAX_VALUE), 23, 13, 0);
    pub(crate) const DIAMOND_USER_SHROUD: Argb =
        Argb(Some(Self::DIMMING_MAX_VALUE), 20, 20, 0);
    pub(crate) const DIAMOND_USER_IDLE: Argb =
        Argb(Some(Self::DIMMING_MAX_VALUE), 18, 23, 18);
    pub(crate) const DIAMOND_USER_QR_SCAN: Argb =
        Argb(Some(Self::DIMMING_MAX_VALUE), 24, 29, 24);
    pub(crate) const DIAMOND_USER_RED: Argb =
        Argb(Some(Self::DIMMING_MAX_VALUE), 30, 2, 0);
    pub(crate) const DIAMOND_USER_SIGNUP: Argb =
        Argb(Some(Self::DIMMING_MAX_VALUE), 31, 36, 31);
    pub(crate) const DIAMOND_USER_FLASH: Argb =
        Argb(Some(Self::DIMMING_MAX_VALUE), 255, 255, 255);

    pub fn is_off(&self) -> bool {
        self.0 == Some(0) || (self.1 == 0 && self.2 == 0 && self.3 == 0)
    }
}
