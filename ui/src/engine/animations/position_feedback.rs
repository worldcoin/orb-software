use crate::engine::{Animation, AnimationState, CenterFrame, RingFrame, Transition};
use orb_rgb::Argb;
use std::{any::Any, f64::consts::PI};

// Maximum vibrancy colors for Diamond hardware
const COLOR_BAD: Argb = Argb(Some(31), 255, 0, 0); // Pure vivid red
const COLOR_MID: Argb = Argb(Some(31), 255, 200, 0); // Bright yellow
const COLOR_GOOD: Argb = Argb(Some(31), 0, 255, 0); // Pure vivid green

/// Brightness tail cutoff. Gaussian values below this are clamped to zero
/// to eliminate muddy "bleeding" tails on the ring arc edges.
const BRIGHTNESS_CUTOFF: f64 = 0.05;

/// Distance (mm) below which angle tracking is fully frozen.
/// Between this and ANGLE_RAMP_END, tracking gradually engages.
const ANGLE_RAMP_START: f64 = 3.0;

/// Distance (mm) above which angle tracking is at full rate.
/// NOTE: must be < center_threshold (15mm) so the angle is stable
/// before the directional arc becomes visible.
const ANGLE_RAMP_END: f64 = 12.0;

/// SmoothDamp smooth time for position (seconds).
/// At 0.01s the spring is near-instant — one frame of C2 damping
/// at 90fps prevents raw quantization steps from being visible.
const POSITION_SMOOTH_TIME: f64 = 0.01;

/// 3-stop color gradient: Red → Yellow → Green
/// Avoids the muddy brown that linear red↔green lerp produces.
fn error_color(error: f64) -> Argb {
    let e = error.clamp(0.0, 1.0);
    if e > 0.5 {
        let t = (1.0 - e) * 2.0;
        COLOR_BAD.lerp(COLOR_MID, t)
    } else {
        let t = (0.5 - e) * 2.0;
        COLOR_MID.lerp(COLOR_GOOD, t)
    }
}

/// Exponential moving average (dt-aware). Used for error, sigma.
fn ema(current: f64, target: f64, rate: f64, dt: f64) -> f64 {
    current + (target - current) * (1.0 - (-rate * dt).exp())
}

/// Shortest angular delta handling 0/2pi wraparound.
fn shortest_angle_delta(from: f64, to: f64) -> f64 {
    let d = to - from;
    (d + PI).rem_euclid(2.0 * PI) - PI
}

/// Angle EMA with wraparound handling.
fn angle_ema(current: f64, target: f64, rate: f64, dt: f64) -> f64 {
    let delta = shortest_angle_delta(current, target);
    let alpha = 1.0 - (-rate * dt).exp();
    (current + delta * alpha).rem_euclid(2.0 * PI)
}

/// Critically-damped spring follower (Unity's Mathf.SmoothDamp).
/// Produces C2-continuous motion: smooth position, velocity AND acceleration.
/// The follower has "inertia" — it coasts through inter-frame gaps instead
/// of creating visible "kicks" like EMA does at each new measurement.
///
/// Returns the new position. Updates `vel` in place.
fn smooth_damp(
    current: f64,
    target: f64,
    vel: &mut f64,
    smooth_time: f64,
    dt: f64,
) -> f64 {
    let smooth_time = smooth_time.max(0.0001);
    let omega = 2.0 / smooth_time;
    let x = omega * dt;
    let exp = 1.0 / (1.0 + x + 0.48 * x * x + 0.235 * x * x * x);
    let change = current - target;
    let temp = (*vel + omega * change) * dt;
    *vel = (*vel - omega * temp) * exp;

    target + (change + temp) * exp
}

// ---------------------------------------------------------------------------
// Ring animation: directional Gaussian arc tracking Y/Z position
// ---------------------------------------------------------------------------

/// Ring LED position feedback with SmoothDamp tracking
/// and super-Gaussian rendering for crisp arcs.
///
/// Uses only Y (horizontal) and Z (vertical) axes for guidance.
/// X (depth) is excluded — the IPD-based depth estimate has ±50-100mm noise,
/// which exceeds usable thresholds and would cause false red signals.
///
/// Position is tracked via a critically-damped spring (SmoothDamp) which
/// provides C2-continuous motion — the arc glides like a heavy object
/// through water, naturally bridging the ~70ms gaps between 15Hz face
/// detection updates without explicit velocity prediction.
pub struct PositionFeedback<const N: usize> {
    target_y: f64,
    target_z: f64,
    smooth_y: f64,
    smooth_z: f64,
    optimal_y: f64,
    optimal_z: f64,

    /// Internal spring velocity — NOT calculated from noisy input.
    /// Maintained by the SmoothDamp integrator for C2 continuity.
    spring_vel_y: f64,
    spring_vel_z: f64,

    current_angle: f64,
    current_sigma: f64,
    current_error: f64,

    angle_rate: f64,
    sigma_rate: f64,
    error_rate: f64,

    min_sigma: f64,
    max_sigma: f64,

    center_threshold: f64,
    far_threshold: f64,

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

            spring_vel_y: 0.0,
            spring_vel_z: 0.0,

            current_angle: 0.0,
            current_sigma: 0.5,
            current_error: 0.5,

            angle_rate: 25.0, // ~40ms — arc snaps to direction instantly
            sigma_rate: 10.0, // ~100ms — arc width adapts fast
            error_rate: 8.0,  // ~125ms — color responds quickly but still smooth

            min_sigma: 0.15,
            max_sigma: PI,

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

        // SmoothDamp: critically-damped spring tracks the raw target.
        // The spring's internal velocity provides natural "coasting"
        // between 15Hz face detection frames — no explicit prediction
        // needed. C2 continuity means no visible kicks or steps.
        self.smooth_y = smooth_damp(
            self.smooth_y,
            self.target_y,
            &mut self.spring_vel_y,
            POSITION_SMOOTH_TIME,
            dt,
        );
        self.smooth_z = smooth_damp(
            self.smooth_z,
            self.target_z,
            &mut self.spring_vel_z,
            POSITION_SMOOTH_TIME,
            dt,
        );

        // Offset from optimal
        let dy = self.smooth_y - self.optimal_y;
        let dz = self.smooth_z - self.optimal_z;
        let distance = (dy * dy + dz * dz).sqrt();

        // Angle tracking with smooth ramp — no hard threshold.
        let direction_confidence = ((distance - ANGLE_RAMP_START)
            / (ANGLE_RAMP_END - ANGLE_RAMP_START))
            .clamp(0.0, 1.0);
        let effective_angle_rate = self.angle_rate * direction_confidence;

        if distance > 1.0 {
            let a = (-dy).atan2(dz);
            let target_angle = if a < 0.0 { a + 2.0 * PI } else { a };
            self.current_angle =
                angle_ema(self.current_angle, target_angle, effective_angle_rate, dt);
        }

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

        // Render: super-Gaussian (x^4) for crisp edges
        let sigma = self.current_sigma;
        let sigma_sq = sigma * sigma;

        for (i, led) in frame.iter_mut().enumerate() {
            let led_angle = (i as f64 / N as f64) * 2.0 * PI;

            let mut ang_dist = (led_angle - self.current_angle).abs();
            if ang_dist > PI {
                ang_dist = 2.0 * PI - ang_dist;
            }

            let x_norm_sq = (ang_dist * ang_dist) / sigma_sq;
            let raw = (-x_norm_sq * x_norm_sq).exp();
            let clipped =
                ((raw - BRIGHTNESS_CUTOFF) / (1.0 - BRIGHTNESS_CUTOFF)).max(0.0);

            // Blend toward uniform when centered. Cubic curve makes
            // the directional arc assert itself quickly as you move
            // off-center — at error=0.3 the arc is already dominant.
            let uw = 1.0 - self.current_error;
            let uniform_weight = uw * uw * uw;
            let brightness = clipped + uniform_weight * (1.0 - clipped);

            // Manual multiply — Argb::mul snaps to OFF when a multi-component
            // color loses a component at low brightness (e.g. near-red (255,16,0)
            // * 0.04 → floor gives (10,0,0) → snapped to black). Bypass that
            // here with round() for better color fidelity at low brightness.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            {
                *led = Argb(
                    color.0,
                    (f64::from(color.1) * brightness).round() as u8,
                    (f64::from(color.2) * brightness).round() as u8,
                    (f64::from(color.3) * brightness).round() as u8,
                );
            }
        }

        if self.frame_count % 180 == 0 {
            tracing::info!(
                "ring_fb: err={:.2} angle={:.0}° sigma={:.2} dist={:.0}mm svel=({:.0},{:.0}) rgb=({},{},{})",
                self.current_error,
                self.current_angle.to_degrees(),
                self.current_sigma,
                distance,
                self.spring_vel_y,
                self.spring_vel_z,
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
/// Shows traffic-light color (red/yellow/green) based on position error.
pub struct PositionFeedbackCenter<const N: usize> {
    target_y: f64,
    target_z: f64,
    smooth_y: f64,
    smooth_z: f64,
    optimal_y: f64,
    optimal_z: f64,

    spring_vel_y: f64,
    spring_vel_z: f64,

    current_error: f64,
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

            spring_vel_y: 0.0,
            spring_vel_z: 0.0,

            current_error: 0.5,
            error_rate: 8.0, // ~125ms — fast color response

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

        self.smooth_y = smooth_damp(
            self.smooth_y,
            self.target_y,
            &mut self.spring_vel_y,
            POSITION_SMOOTH_TIME,
            dt,
        );
        self.smooth_z = smooth_damp(
            self.smooth_z,
            self.target_z,
            &mut self.spring_vel_z,
            POSITION_SMOOTH_TIME,
            dt,
        );

        let dy = self.smooth_y - self.optimal_y;
        let dz = self.smooth_z - self.optimal_z;
        let distance = (dy * dy + dz * dz).sqrt();

        let target_error = ((distance - self.center_threshold)
            / (self.far_threshold - self.center_threshold))
            .clamp(0.0, 1.0);
        self.current_error = ema(self.current_error, target_error, self.error_rate, dt);

        let color = error_color(self.current_error);
        for led in frame.iter_mut() {
            *led = color;
        }

        if self.frame_count % 180 == 0 {
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
