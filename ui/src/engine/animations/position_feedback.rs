use crate::engine::{Animation, AnimationState, CenterFrame, RingFrame, Transition};
use orb_rgb::Argb;
use std::{any::Any, f64::consts::PI};

// Maximum vibrancy colors for Diamond hardware
const COLOR_BAD: Argb = Argb(Some(31), 255, 0, 0); // Pure vivid red
const COLOR_MID: Argb = Argb(Some(31), 255, 200, 0); // Bright yellow
const COLOR_GOOD: Argb = Argb(Some(31), 0, 255, 0); // Pure vivid green

/// 3-stop color gradient: Red → Yellow → Green
/// Avoids the muddy brown that linear red↔green lerp produces.
fn error_color(error: f64) -> Argb {
    let e = error.clamp(0.0, 1.0);
    if e > 0.5 {
        // Red → Yellow (error 1.0→0.5)
        let t = (1.0 - e) * 2.0; // 0→1 as error goes 1.0→0.5
        COLOR_BAD.lerp(COLOR_MID, t)
    } else {
        // Yellow → Green (error 0.5→0.0)
        let t = (0.5 - e) * 2.0; // 0→1 as error goes 0.5→0.0
        COLOR_MID.lerp(COLOR_GOOD, t)
    }
}

/// Exponential moving average (dt-aware).
fn ema(current: f64, target: f64, rate: f64, dt: f64) -> f64 {
    current + (target - current) * (1.0 - (-rate * dt).exp())
}

/// Shortest angular delta handling 0/2pi wraparound.
fn shortest_angle_delta(from: f64, to: f64) -> f64 {
    let d = to - from;
    (d + PI).rem_euclid(2.0 * PI) - PI
}

// ---------------------------------------------------------------------------
// Ring animation: directional Gaussian arc tracking Y/Z position
// ---------------------------------------------------------------------------

/// Ring LED position feedback with SLERP + Gaussian falloff.
///
/// Uses only Y (horizontal) and Z (vertical) axes for guidance.
/// X (depth) is excluded — the IPD-based depth estimate has ±50-100mm noise,
/// which exceeds usable thresholds and would cause false red signals.
pub struct PositionFeedback<const N: usize> {
    target_y: f64,
    target_z: f64,
    smooth_y: f64,
    smooth_z: f64,
    optimal_y: f64,
    optimal_z: f64,

    current_angle: f64,
    current_sigma: f64,
    current_error: f64,

    position_rate: f64,
    angle_rate: f64,
    sigma_rate: f64,
    error_rate: f64,

    min_sigma: f64,
    max_sigma: f64,

    center_threshold: f64,
    far_threshold: f64,
    brightness_floor: f64,

    frame_count: u32,
    has_position: bool,
}

impl<const N: usize> PositionFeedback<N> {
    pub fn new(_color: Argb) -> Self {
        Self {
            target_y: 0.0,
            target_z: 80.0,
            smooth_y: 0.0,
            smooth_z: 80.0,
            optimal_y: -15.0,
            optimal_z: 80.0,

            current_angle: 0.0,
            current_sigma: 0.5,
            current_error: 0.5,

            position_rate: 10.0,
            angle_rate: 8.0,
            sigma_rate: 4.0,
            error_rate: 5.0,

            min_sigma: 0.12,
            max_sigma: PI,

            center_threshold: 15.0,
            far_threshold: 80.0,
            brightness_floor: 0.02,

            frame_count: 0,
            has_position: false,
        }
    }

    pub fn update_position(&mut self, _x: f64, y: f64, z: f64) {
        if !self.has_position {
            self.smooth_y = y;
            self.smooth_z = z;
        }
        self.target_y = y;
        self.target_z = z;
        self.has_position = true;
    }
}

impl<const N: usize> Animation for PositionFeedback<N> {
    type Frame = RingFrame<N>;

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn animate(&mut self, frame: &mut RingFrame<N>, dt: f64, idle: bool) -> AnimationState {
        if idle || !self.has_position {
            return AnimationState::Running;
        }

        self.frame_count += 1;

        // Smooth position
        self.smooth_y = ema(self.smooth_y, self.target_y, self.position_rate, dt);
        self.smooth_z = ema(self.smooth_z, self.target_z, self.position_rate, dt);

        // Offset from optimal
        let dy = self.smooth_y - self.optimal_y;
        let dz = self.smooth_z - self.optimal_z;
        let distance = (dy * dy + dz * dz).sqrt();

        // Target angle (where user IS on the ring)
        let target_angle = if distance > 1.0 {
            let a = (-dy).atan2(dz);

            if a < 0.0 { a + 2.0 * PI } else { a }
        } else {
            self.current_angle
        };

        // SLERP: smooth angle via shortest path
        let delta = shortest_angle_delta(self.current_angle, target_angle);
        self.current_angle += delta * (1.0 - (-self.angle_rate * dt).exp());
        self.current_angle = self.current_angle.rem_euclid(2.0 * PI);

        // Error: 0 = centered, 1 = far
        let target_error = ((distance - self.center_threshold)
            / (self.far_threshold - self.center_threshold))
            .clamp(0.0, 1.0);
        self.current_error = ema(self.current_error, target_error, self.error_rate, dt);

        // Arc width: wide when close, narrow when far
        let target_sigma =
            self.max_sigma - self.current_error * (self.max_sigma - self.min_sigma);
        self.current_sigma = ema(self.current_sigma, target_sigma, self.sigma_rate, dt);

        // Color: 3-stop red → yellow → green
        let color = error_color(self.current_error);

        // Render: Gaussian falloff per LED
        let two_sigma_sq = 2.0 * self.current_sigma * self.current_sigma;

        for (i, led) in frame.iter_mut().enumerate() {
            let led_angle = (i as f64 / N as f64) * 2.0 * PI;

            let mut ang_dist = (led_angle - self.current_angle).abs();
            if ang_dist > PI {
                ang_dist = 2.0 * PI - ang_dist;
            }

            let brightness = (-ang_dist * ang_dist / two_sigma_sq).exp();

            if brightness < self.brightness_floor {
                *led = Argb::OFF;
            } else {
                *led = color * brightness;
            }
        }

        if self.frame_count % 60 == 0 {
            tracing::info!(
                "ring_fb: err={:.2} angle={:.0}° sigma={:.2} dist={:.0}mm rgb=({},{},{})",
                self.current_error,
                self.current_angle.to_degrees(),
                self.current_sigma,
                distance,
                color.1,
                color.2,
                color.3,
            );
        }

        AnimationState::Running
    }

    fn stop(&mut self, _transition: Transition) -> eyre::Result<()> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Center animation: uniform fill matching ring error color (traffic light)
// ---------------------------------------------------------------------------

/// Center LED position feedback — uniform color fill.
///
/// Acts as a "traffic light" reinforcing the ring's directional guidance.
/// Red = off-center, Yellow = getting close, Green = correctly positioned.
pub struct PositionFeedbackCenter<const N: usize> {
    target_y: f64,
    target_z: f64,
    smooth_y: f64,
    smooth_z: f64,
    optimal_y: f64,
    optimal_z: f64,

    current_error: f64,

    position_rate: f64,
    error_rate: f64,

    center_threshold: f64,
    far_threshold: f64,

    frame_count: u32,
    has_position: bool,
}

impl<const N: usize> PositionFeedbackCenter<N> {
    pub fn new() -> Self {
        Self {
            target_y: 0.0,
            target_z: 80.0,
            smooth_y: 0.0,
            smooth_z: 80.0,
            optimal_y: -15.0,
            optimal_z: 80.0,

            current_error: 0.5,

            position_rate: 10.0,
            error_rate: 5.0,

            center_threshold: 15.0,
            far_threshold: 80.0,

            frame_count: 0,
            has_position: false,
        }
    }

    pub fn update_position(&mut self, _x: f64, y: f64, z: f64) {
        if !self.has_position {
            self.smooth_y = y;
            self.smooth_z = z;
        }
        self.target_y = y;
        self.target_z = z;
        self.has_position = true;
    }
}

impl<const N: usize> Animation for PositionFeedbackCenter<N> {
    type Frame = CenterFrame<N>;

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn animate(
        &mut self,
        frame: &mut CenterFrame<N>,
        dt: f64,
        idle: bool,
    ) -> AnimationState {
        if idle || !self.has_position {
            return AnimationState::Running;
        }

        self.frame_count += 1;

        // Same smoothing + error calculation as ring (keeps them in sync)
        self.smooth_y = ema(self.smooth_y, self.target_y, self.position_rate, dt);
        self.smooth_z = ema(self.smooth_z, self.target_z, self.position_rate, dt);

        let dy = self.smooth_y - self.optimal_y;
        let dz = self.smooth_z - self.optimal_z;
        let distance = (dy * dy + dz * dz).sqrt();

        let target_error = ((distance - self.center_threshold)
            / (self.far_threshold - self.center_threshold))
            .clamp(0.0, 1.0);
        self.current_error = ema(self.current_error, target_error, self.error_rate, dt);

        // Uniform fill: same 3-stop color as ring
        let color = error_color(self.current_error);

        for led in frame.iter_mut() {
            *led = color;
        }

        if self.frame_count % 60 == 0 {
            tracing::info!(
                "center_fb: err={:.2} dist={:.0}mm rgb=({},{},{})",
                self.current_error,
                distance,
                color.1,
                color.2,
                color.3,
            );
        }

        AnimationState::Running
    }

    fn stop(&mut self, _transition: Transition) -> eyre::Result<()> {
        Ok(())
    }
}
