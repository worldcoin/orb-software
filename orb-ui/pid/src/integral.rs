//! Integrals.

use std::ops::Range;

/// Approximation of the integral using Riemann sum. See
/// <https://en.wikipedia.org/wiki/Riemann_sum>.
#[derive(Clone, Default, Debug)]
pub struct RiemannSum {
    sum: f64,
}

impl RiemannSum {
    /// Adds a new partition of the target function. Returns the current
    /// integral value.
    pub fn add(&mut self, x: f64, dt: f64) -> f64 {
        self.sum += x * dt;
        self.sum
    }

    /// Resets the sum.
    pub fn reset(&mut self) {
        self.sum = 0.0;
    }
}

/// Calculates definite integral over given `interval` of function `f`.
pub fn integrate(interval: Range<f64>, step: f64, f: impl Fn(f64) -> f64) -> f64 {
    let mut integral = RiemannSum::default();
    let mut t = interval.start;
    let mut x = 0.0;
    while t < interval.end {
        x = integral.add(f(t), step);
        t += step;
    }
    x
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    #[test]
    fn test_variable() {
        // Definite integral from 1 to 2 of 2x dx is 3.
        assert_abs_diff_eq!(
            integrate(1.0..2.0, 0.001, |x| 2.0 * x),
            3.0,
            epsilon = 0.01
        );
    }

    #[test]
    fn test_trigonometry() {
        // Definite integral from 1 to 3 of cos(x) dx is -0.7.
        assert_abs_diff_eq!(
            integrate(1.0..3.0, 0.001, f64::cos),
            -0.7,
            epsilon = 0.001
        );
    }
}
