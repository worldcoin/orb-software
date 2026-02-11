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

// ---------------------------------------------------------------------------
// One Euro Filter — adaptive low-pass with velocity-dependent cutoff
// ---------------------------------------------------------------------------

/// Compute the smoothing factor alpha for a given cutoff frequency.
/// alpha = dt / (dt + tau) where tau = 1 / (2 * pi * fc)
fn smoothing_factor(dt: f64, cutoff: f64) -> f64 {
    let tau = 1.0 / (2.0 * PI * cutoff);
    dt / (dt + tau)
}

/// One Euro Filter (Casiez et al. 2012).
///
/// Adaptive low-pass filter that dynamically adjusts cutoff based on
/// input velocity:
/// - **Stationary** → low cutoff → heavy smoothing → no jitter
/// - **Moving fast** → high cutoff → near-zero lag → instant response
///
/// This gives the "water" feel: perfectly still when you're still,
/// flows instantly when you move.
struct OneEuroFilter {
    x_prev: f64,
    dx_prev: f64,
    /// Minimum cutoff frequency (Hz) when stationary. Lower = smoother.
    min_cutoff: f64,
    /// Speed coefficient. Higher = more responsive to fast movements.
    beta: f64,
    /// Cutoff frequency (Hz) for the derivative (velocity) filter.
    d_cutoff: f64,
    initialized: bool,
}

impl OneEuroFilter {
    fn new(min_cutoff: f64, beta: f64, d_cutoff: f64) -> Self {
        Self {
            x_prev: 0.0,
            dx_prev: 0.0,
            min_cutoff,
            beta,
            d_cutoff,
            initialized: false,
        }
    }

    fn filter(&mut self, x: f64, dt: f64) -> f64 {
        if dt <= 0.0 {
            return self.x_prev;
        }

        if !self.initialized {
            self.x_prev = x;
            self.dx_prev = 0.0;
            self.initialized = true;

            return x;
        }

        // Estimate velocity from raw input
        let dx = (x - self.x_prev) / dt;

        // Smooth the velocity estimate
        let alpha_d = smoothing_factor(dt, self.d_cutoff);
        let dx_hat = alpha_d * dx + (1.0 - alpha_d) * self.dx_prev;

        // Adaptive cutoff: fast movement → high cutoff → low lag
        let cutoff = self.min_cutoff + self.beta * dx_hat.abs();

        // Smooth the position with the adaptive cutoff
        let alpha = smoothing_factor(dt, cutoff);
        let x_hat = alpha * x + (1.0 - alpha) * self.x_prev;

        self.x_prev = x_hat;
        self.dx_prev = dx_hat;

        x_hat
    }
}

// ---------------------------------------------------------------------------
// Ring animation: directional Gaussian arc tracking Y/Z position
// ---------------------------------------------------------------------------

/// Ring LED position feedback with One Euro Filter tracking
/// and super-Gaussian rendering for crisp arcs.
///
/// Tracks Y (horizontal), Z (vertical), and X (depth) axes.
/// Y/Z drive the directional arc and arc width; X drives brightness
/// vibrancy and gates the color (prevents false green when depth
/// is wrong). X uses heavier filtering to handle ±50-100mm noise.
///
/// Position is tracked via One Euro Filters which provide adaptive
/// smoothing: instant response when moving, rock-solid stability when still.
pub struct PositionFeedback<const N: usize> {
    target_x: f64,
    target_y: f64,
    target_z: f64,
    filter_x: OneEuroFilter,
    filter_y: OneEuroFilter,
    filter_z: OneEuroFilter,
    optimal_x: f64,
    optimal_y: f64,
    optimal_z: f64,

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

    current_depth_error: f64,
    current_depth_vibrancy: f64,
    depth_error_rate: f64,
    depth_vibrancy_rate: f64,
    depth_good_range: f64,
    depth_dim_range: f64,
    min_vibrancy: f64,

    frame_count: u32,
    has_position: bool,
}

impl<const N: usize> PositionFeedback<N> {
    pub fn new(_color: Argb) -> Self {
        // Y/Z One Euro: face tracking in mm
        let min_cutoff = 1.5;
        let beta = 0.1;
        let d_cutoff = 1.0;

        // Depth (X) One Euro: LOWER beta than Y/Z because X is noisier (±50-100mm).
        // High beta + high noise = cutoff blows to 300+ Hz (above 15Hz Nyquist),
        // effectively disabling the filter. beta=0.01 means cutoff only reaches
        // ~5Hz at 300mm/s real movement, ignoring noise velocity spikes.
        // Higher min_cutoff (2.0Hz) compensates — faster base tracking.
        // Higher d_cutoff (2.0Hz) detects real movement ~80ms faster.
        let depth_min_cutoff = 2.0;
        let depth_beta = 0.01;
        let depth_d_cutoff = 2.0;

        Self {
            target_x: 0.0,
            target_y: 0.0,
            target_z: 80.0,
            filter_x: OneEuroFilter::new(depth_min_cutoff, depth_beta, depth_d_cutoff),
            filter_y: OneEuroFilter::new(min_cutoff, beta, d_cutoff),
            filter_z: OneEuroFilter::new(min_cutoff, beta, d_cutoff),
            optimal_x: 500.0,
            optimal_y: -15.0,
            optimal_z: 80.0,

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

            current_depth_error: 0.5,
            current_depth_vibrancy: 1.0,
            depth_error_rate: 10.0,   // ~100ms — decisive color transitions
            depth_vibrancy_rate: 4.0, // ~250ms — smooth "breathing" brightness, no flicker
            depth_good_range: 100.0,  // ±100mm plateau
            depth_dim_range: 300.0,   // ±300mm decay
            min_vibrancy: 0.1,

            frame_count: 0,
            has_position: false,
        }
    }

    pub fn update_position(&mut self, x: f64, y: f64, z: f64) {
        self.target_x = x;
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

        // One Euro Filter: adaptive smoothing based on velocity.
        // Moving fast → near-zero lag. Holding still → heavy smoothing.
        let smooth_y = self.filter_y.filter(self.target_y, dt);
        let smooth_z = self.filter_z.filter(self.target_z, dt);

        // Depth: heavy One Euro filtering for noisy X axis
        let smooth_x = self.filter_x.filter(self.target_x, dt);
        let depth_delta = (smooth_x - self.optimal_x).abs();
        let depth_t = ((depth_delta - self.depth_good_range)
            / self.depth_dim_range)
            .clamp(0.0, 1.0);

        // Depth error gates color — prevents false green when depth is wrong
        self.current_depth_error =
            ema(self.current_depth_error, depth_t, self.depth_error_rate, dt);

        // Depth vibrancy — brightness dims away from optimal distance
        let target_vibrancy = 1.0 - depth_t * (1.0 - self.min_vibrancy);
        self.current_depth_vibrancy = ema(
            self.current_depth_vibrancy,
            target_vibrancy,
            self.depth_vibrancy_rate,
            dt,
        );

        // Offset from optimal
        let dy = smooth_y - self.optimal_y;
        let dz = smooth_z - self.optimal_z;
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
        // Combined error: max of Y/Z centering and depth — prevents false green
        let color_error = self.current_error.max(self.current_depth_error);
        let color = error_color(color_error);

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
            let brightness =
                (clipped + uniform_weight * (1.0 - clipped)) * self.current_depth_vibrancy;

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
                "ring_fb: err={:.2} depth_err={:.2} vib={:.2} angle={:.0}° sigma={:.2} dist={:.0}mm rgb=({},{},{})",
                self.current_error,
                self.current_depth_error,
                self.current_depth_vibrancy,
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
/// Shows traffic-light color (red/yellow/green) based on position error.
/// Depth (X) gates the color and dims brightness, matching the ring.
pub struct PositionFeedbackCenter<const N: usize> {
    target_x: f64,
    target_y: f64,
    target_z: f64,
    filter_x: OneEuroFilter,
    filter_y: OneEuroFilter,
    filter_z: OneEuroFilter,
    optimal_x: f64,
    optimal_y: f64,
    optimal_z: f64,

    current_error: f64,
    error_rate: f64,

    center_threshold: f64,
    far_threshold: f64,

    current_depth_error: f64,
    current_depth_vibrancy: f64,
    depth_error_rate: f64,
    depth_vibrancy_rate: f64,
    depth_good_range: f64,
    depth_dim_range: f64,
    min_vibrancy: f64,

    frame_count: u32,
    has_position: bool,
}

impl<const N: usize> PositionFeedbackCenter<N> {
    pub fn new() -> Self {
        let min_cutoff = 1.5;
        let beta = 0.1;
        let d_cutoff = 1.0;
        let depth_min_cutoff = 2.0;
        let depth_beta = 0.01;
        let depth_d_cutoff = 2.0;

        Self {
            target_x: 0.0,
            target_y: 0.0,
            target_z: 80.0,
            filter_x: OneEuroFilter::new(depth_min_cutoff, depth_beta, depth_d_cutoff),
            filter_y: OneEuroFilter::new(min_cutoff, beta, d_cutoff),
            filter_z: OneEuroFilter::new(min_cutoff, beta, d_cutoff),
            optimal_x: 500.0,
            optimal_y: -15.0,
            optimal_z: 80.0,

            current_error: 0.5,
            error_rate: 8.0, // ~125ms — fast color response

            center_threshold: 15.0,
            far_threshold: 80.0,

            current_depth_error: 0.5,
            current_depth_vibrancy: 1.0,
            depth_error_rate: 10.0,
            depth_vibrancy_rate: 4.0,
            depth_good_range: 100.0,
            depth_dim_range: 300.0,
            min_vibrancy: 0.1,

            frame_count: 0,
            has_position: false,
        }
    }

    pub fn update_position(&mut self, x: f64, y: f64, z: f64) {
        self.target_x = x;
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

        let smooth_y = self.filter_y.filter(self.target_y, dt);
        let smooth_z = self.filter_z.filter(self.target_z, dt);

        // Depth filtering + vibrancy
        let smooth_x = self.filter_x.filter(self.target_x, dt);
        let depth_delta = (smooth_x - self.optimal_x).abs();
        let depth_t = ((depth_delta - self.depth_good_range)
            / self.depth_dim_range)
            .clamp(0.0, 1.0);
        self.current_depth_error =
            ema(self.current_depth_error, depth_t, self.depth_error_rate, dt);
        let target_vibrancy = 1.0 - depth_t * (1.0 - self.min_vibrancy);
        self.current_depth_vibrancy = ema(
            self.current_depth_vibrancy,
            target_vibrancy,
            self.depth_vibrancy_rate,
            dt,
        );

        let dy = smooth_y - self.optimal_y;
        let dz = smooth_z - self.optimal_z;
        let distance = (dy * dy + dz * dz).sqrt();

        let target_error = ((distance - self.center_threshold)
            / (self.far_threshold - self.center_threshold))
            .clamp(0.0, 1.0);
        self.current_error = ema(self.current_error, target_error, self.error_rate, dt);

        let color_error = self.current_error.max(self.current_depth_error);
        let color = error_color(color_error);
        let vibrancy = self.current_depth_vibrancy;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        for led in frame.iter_mut() {
            *led = Argb(
                color.0,
                (f64::from(color.1) * vibrancy).round() as u8,
                (f64::from(color.2) * vibrancy).round() as u8,
                (f64::from(color.3) * vibrancy).round() as u8,
            );
        }

        if self.frame_count % 180 == 0 {
            tracing::info!(
                "center_fb: err={:.2} depth_err={:.2} vib={:.2} dist={:.0}mm rgb=({},{},{})",
                self.current_error,
                self.current_depth_error,
                self.current_depth_vibrancy,
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
