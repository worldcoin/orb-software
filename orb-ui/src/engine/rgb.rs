use serde::{Deserialize, Serialize};
use std::ops;

/// RGB LED color.
#[derive(Eq, PartialEq, Copy, Clone, Default, Debug, Serialize, Deserialize)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl ops::Mul<f64> for Rgb {
    type Output = Self;

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn mul(self, rhs: f64) -> Self::Output {
        Rgb(
            (f64::from(self.0) * rhs) as u8,
            (f64::from(self.1) * rhs) as u8,
            (f64::from(self.2) * rhs) as u8,
        )
    }
}

impl ops::MulAssign<f64> for Rgb {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn mul_assign(&mut self, rhs: f64) {
        self.0 = (f64::from(self.0) * rhs) as u8;
        self.1 = (f64::from(self.1) * rhs) as u8;
        self.2 = (f64::from(self.2) * rhs) as u8;
    }
}

impl ops::Add for Rgb {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Rgb(self.0 + rhs.0, self.1 + rhs.1, self.2 + rhs.2)
    }
}

#[allow(missing_docs)]
impl Rgb {
    pub(crate) const OFF: Rgb = Rgb(0, 0, 0);
    pub(crate) const OPERATOR_AMBER: Rgb = Rgb(20, 16, 0);
    // To help quickly distinguish dev vs prod software,
    // the default operator LED color is white for prod, yellow for dev
    pub(crate) const OPERATOR_DEFAULT: Rgb = {
        #[cfg(not(feature = "stage"))]
        {
            Rgb(20, 20, 20)
        }
        #[cfg(feature = "stage")]
        {
            Rgb(8, 25, 8)
        }
    };
    pub(crate) const OPERATOR_VERSIONS_DEPRECATED: Rgb = Rgb(128, 128, 0);
    pub(crate) const OPERATOR_VERSIONS_OUTDATED: Rgb = Rgb(255, 0, 0);
    pub(crate) const USER_AMBER: Rgb = Rgb(23, 13, 0);
    pub(crate) const USER_SHROUD_DIAMOND: Rgb = Rgb(20, 20, 0);
    pub(crate) const USER_IDLE: Rgb = Rgb(18, 18, 18);
    pub(crate) const USER_QR_SCAN: Rgb = Rgb(24, 24, 24);
    pub(crate) const USER_RED: Rgb = Rgb(30, 2, 0);
    pub(crate) const USER_SIGNUP: Rgb = Rgb(31, 31, 31);
}
