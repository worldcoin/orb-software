//! Derivatives.

/// Infinite impulse response filter. Digital simulation of a simple low-pass RC
/// filter.
#[derive(Clone, Default, Debug)]
pub struct LowPassFilter {
    prev_x: Option<f64>,
}

impl LowPassFilter {
    /// Adds a new partition of the target function. Returns the filtered value.
    pub fn add(&mut self, x: f64, dt: f64, rc: f64) -> f64 {
        *self.prev_x.insert(self.prev_x.map_or(x, |prev_x| {
            let alpha = if dt == 0.0 { 0.0 } else { dt / (rc + dt) };
            prev_x + alpha * (x - prev_x)
        }))
    }

    /// Adds a new partition of the target function. Returns the current slope
    /// of the filtered target function.
    pub fn add_slope(&mut self, mut x: f64, dt: f64, rc: f64) -> Option<f64> {
        let prev_x = self.prev_x;
        x = self.add(x, dt, rc);
        prev_x.and_then(|prev_x| (dt != 0.0).then_some((x - prev_x) / dt))
    }

    /// Resets the filter.
    pub fn reset(&mut self) {
        self.prev_x = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    #[test]
    fn test_oscillation() {
        const DT: f64 = 0.01;
        const RC: f64 = DT * 250.0;
        // Test that this function has constant slope of 1. sin(x) is filtered
        // out and x/dx is 1.
        let f = |x: f64| (x * 1500.0).sin() * 0.5 + x;
        let mut derivative = LowPassFilter::default();
        let mut t = 0.0;
        while t < 10.0 {
            let x = f(t);
            if let Some(slope) = derivative.add_slope(x, DT, RC) {
                // println!("t={t:.03} x={x:.03} slope={slope:.03}");
                if t > 3.5 {
                    assert_abs_diff_eq!(slope, 1.0, epsilon = 0.5);
                }
            }
            t += DT;
        }
    }
}
