use crate::engine::{Animation, AnimationState, CenterFrame, RingFrame, Transition};
use orb_rgb::Argb;
use std::time::Instant;
use std::{any::Any, f64::consts::PI};

/// LED dimming value for Diamond hardware.
const DIMMING: Option<u8> = Some(31);


/// Crossfade with extended dim-red zone:
///   e 1.0→0.6  bright red → dim red
///   e 0.6→0.25 dim red (long plateau)
///   e 0.25→0.0 dim green → bright green (continuous ramp, no plateau)
/// "Too close" is handled by depth_error feeding into color_error.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn error_color(error: f64) -> Argb {
    const DIM: f64 = 0.25;
    let e = error.clamp(0.0, 1.0);
    if e > 0.6 {
        let t = (e - 0.6) / 0.4;
        let intensity = DIM + (1.0 - DIM) * t;
        Argb(DIMMING, (255.0 * intensity).round() as u8, 0, 0)
    } else if e > 0.25 {
        Argb(DIMMING, (255.0 * DIM).round() as u8, 0, 0)
    } else {
        // Continuous ramp: dim green at e=0.25 → bright green at e=0.0
        let t = (0.25 - e) / 0.25;
        let intensity = DIM + (1.0 - DIM) * t;
        Argb(DIMMING, 0, (255.0 * intensity).round() as u8, 0)
    }
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

/// Exponential moving average (dt-aware). Used for error, sigma.
fn ema(current: f64, target: f64, rate: f64, dt: f64) -> f64 {
    current + (target - current) * (1.0 - (-rate * dt).exp())
}


// ---------------------------------------------------------------------------
// Median filter — kills single-frame shot noise before One Euro sees it
// ---------------------------------------------------------------------------

/// 3-tap median filter. Returns the median of the last 3 samples.
/// Adds exactly 0 frames of latency (outputs immediately) but rejects
/// single-frame spikes that would otherwise fool One Euro's velocity
/// estimate into blowing up the cutoff frequency.
struct MedianFilter3 {
    buf: [f64; 3],
    idx: usize,
    count: usize,
}

impl MedianFilter3 {
    fn new() -> Self {
        Self {
            buf: [0.0; 3],
            idx: 0,
            count: 0,
        }
    }

    fn filter(&mut self, x: f64) -> f64 {
        self.buf[self.idx] = x;
        self.idx = (self.idx + 1) % 3;
        self.count = self.count.min(2) + 1;

        if self.count < 3 {
            return x;
        }

        let [a, b, c] = self.buf;
        // Median of 3: the one that's neither min nor max
        if (a >= b && a <= c) || (a <= b && a >= c) {
            a
        } else if (b >= a && b <= c) || (b <= a && b >= c) {
            b
        } else {
            c
        }
    }
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

/// Ring LED position feedback — green fill ring (Face ID style).
///
/// The ring fills with green as the user centers themselves.
/// Fill grows symmetrically from the bottom, meeting at the top.
/// Depth (X) modulates brightness. No red on the ring — center
/// LEDs handle distance feedback separately.
pub struct PositionFeedback<const N: usize> {
    target_x: f64,
    target_y: f64,
    target_z: f64,
    median_x: MedianFilter3,
    filter_x: OneEuroFilter,
    filter_y: OneEuroFilter,
    filter_z: OneEuroFilter,
    optimal_x: f64,
    optimal_y: f64,
    optimal_z: f64,

    current_fill: f64,
    fill_origin: f64,
    error_rate: f64,
    origin_rate: f64,

    center_threshold: f64,
    far_threshold: f64,

    current_depth_vibrancy: f64,
    depth_vibrancy_rate: f64,
    depth_good_range: f64,
    depth_dim_range: f64,
    min_vibrancy: f64,

    // Dead reckoning: predict Y/Z forward to compensate for ML latency
    velocity_y: f64,
    velocity_z: f64,
    last_update: Instant,

    frame_count: u32,
    has_position: bool,
}

impl<const N: usize> PositionFeedback<N> {
    pub fn new(_color: Argb) -> Self {
        // Y/Z One Euro: face tracking in mm
        let min_cutoff = 1.5;
        let beta = 0.1;
        let d_cutoff = 1.0;

        // Depth (X) pipeline: Median → One Euro → EMA
        // Median kills single-frame noise spikes so One Euro sees clean velocity.
        // With clean input, min_cutoff=3.0Hz gives fast base tracking (~50ms),
        // beta=0.02 adds velocity boost without blowing up on noise,
        // and d_cutoff=2.0Hz detects real movement quickly.
        let depth_min_cutoff = 3.0;
        let depth_beta = 0.02;
        let depth_d_cutoff = 2.0;

        Self {
            target_x: 0.0,
            target_y: 0.0,
            target_z: 80.0,
            median_x: MedianFilter3::new(),
            filter_x: OneEuroFilter::new(depth_min_cutoff, depth_beta, depth_d_cutoff),
            filter_y: OneEuroFilter::new(min_cutoff, beta, d_cutoff),
            filter_z: OneEuroFilter::new(min_cutoff, beta, d_cutoff),
            optimal_x: 355.0,
            optimal_y: -15.0,
            optimal_z: 80.0,

            current_fill: 0.0,
            fill_origin: 0.0,
            error_rate: 8.0,  // ~125ms — fill responds quickly but still smooth
            origin_rate: 15.0, // ~67ms — origin tracks user direction quickly

            center_threshold: 15.0,
            far_threshold: 80.0,

            current_depth_vibrancy: 1.0,
            depth_vibrancy_rate: 10.0, // ~100ms — fast brightness, median keeps it clean
            depth_good_range: 100.0,   // ±100mm plateau
            depth_dim_range: 300.0,    // ±300mm decay
            min_vibrancy: 0.1,

            velocity_y: 0.0,
            velocity_z: 0.0,
            last_update: Instant::now(),

            frame_count: 0,
            has_position: false,
        }
    }

    pub fn update_position(&mut self, x: f64, y: f64, z: f64) {
        if self.has_position {
            let elapsed = self.last_update.elapsed().as_secs_f64();
            if elapsed > 0.01 {
                self.velocity_y = (y - self.target_y) / elapsed;
                self.velocity_z = (z - self.target_z) / elapsed;
            }
        }
        self.target_x = x;
        self.target_y = y;
        self.target_z = z;
        self.last_update = Instant::now();
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

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn animate(&mut self, frame: &mut RingFrame<N>, dt: f64, idle: bool) -> AnimationState {
        if idle || !self.has_position {
            return AnimationState::Running;
        }

        self.frame_count += 1;

        // Dead reckoning: predict Y/Z forward to compensate for ML latency.
        let time_since = self.last_update.elapsed().as_secs_f64().min(0.12);
        let predicted_y = self.target_y + self.velocity_y * time_since;
        let predicted_z = self.target_z + self.velocity_z * time_since;

        // One Euro Filter: adaptive smoothing on predicted position.
        let smooth_y = self.filter_y.filter(predicted_y, dt);
        let smooth_z = self.filter_z.filter(predicted_z, dt);

        // Depth: median kills shot noise, then One Euro smooths adaptively
        let median_x = self.median_x.filter(self.target_x);
        let smooth_x = self.filter_x.filter(median_x, dt);
        let depth_delta = (smooth_x - self.optimal_x).abs();
        let depth_t = ((depth_delta - self.depth_good_range)
            / self.depth_dim_range)
            .clamp(0.0, 1.0);

        // Depth vibrancy — brightness dims away from optimal distance
        let target_vibrancy = 1.0 - depth_t * (1.0 - self.min_vibrancy);
        self.current_depth_vibrancy = ema(
            self.current_depth_vibrancy,
            target_vibrancy,
            self.depth_vibrancy_rate,
            dt,
        );

        // Offset from optimal center
        let dy = smooth_y - self.optimal_y;
        let dz = smooth_z - self.optimal_z;
        let distance = (dy * dy + dz * dz).sqrt();

        // Fill origin: the point on the ring closest to the user.
        // This is the direction FROM center TO user, mapped to ring angles.
        // Ring convention: 0 = top, π/2 = right, π = bottom, 3π/2 = left.
        // Only update when the user is far enough for direction to be meaningful.
        if distance > 5.0 {
            let a = dy.atan2(-dz);
            let target_origin = if a < 0.0 { a + 2.0 * PI } else { a };
            self.fill_origin =
                angle_ema(self.fill_origin, target_origin, self.origin_rate, dt);
        }

        // Fill fraction: 0 = far off, 1 = centered
        let target_fill = (1.0
            - ((distance - self.center_threshold)
                / (self.far_threshold - self.center_threshold))
                .clamp(0.0, 1.0))
            .clamp(0.0, 1.0);
        self.current_fill = ema(self.current_fill, target_fill, self.error_rate, dt);

        // Fill angle: how far the green extends from the origin.
        // Power curve makes the fill stingy — the ring only completes
        // when truly centered. At 90% centered the ring is only ~73% full.
        let shaped_fill = self.current_fill * self.current_fill * self.current_fill;
        let fill_half_angle = shaped_fill * PI;

        // Soft edge width in radians (~3 LEDs on a 120-LED ring).
        let edge_width = 2.0 * PI / N as f64 * 3.0;

        for (i, led) in frame.iter_mut().enumerate() {
            let led_angle = (i as f64 / N as f64) * 2.0 * PI;
            let mut dist_from_origin =
                (led_angle - self.fill_origin).abs();
            if dist_from_origin > PI {
                dist_from_origin = 2.0 * PI - dist_from_origin;
            }

            // Smooth falloff at the fill edge. Off where unfilled.
            // Ring always at full brightness — no distance dimming.
            let fade = ((fill_half_angle - dist_from_origin) / edge_width)
                .clamp(0.0, 1.0);
            let g = (255.0 * fade).round() as u8;
            *led = Argb(DIMMING, 0, g, 0);
        }

        if self.frame_count % 180 == 0 {
            tracing::info!(
                "ring_fb: fill={:.2} origin={:.0}° vib={:.2} dist={:.0}mm",
                self.current_fill,
                self.fill_origin.to_degrees(),
                self.current_depth_vibrancy,
                distance,
            );
        }

        AnimationState::Running
    }

    fn stop(&mut self, _transition: Transition) -> eyre::Result<()> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Center animation: uniform fill matching ring error color
// ---------------------------------------------------------------------------

/// Center LED position feedback — uniform color fill.
/// Shows color (red/white/green) based on position error.
/// Depth (X) gates the color and dims brightness, matching the ring.
pub struct PositionFeedbackCenter<const N: usize> {
    target_x: f64,
    target_y: f64,
    target_z: f64,
    median_x: MedianFilter3,
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

    velocity_y: f64,
    velocity_z: f64,
    last_update: Instant,

    frame_count: u32,
    has_position: bool,
}

impl<const N: usize> PositionFeedbackCenter<N> {
    pub fn new() -> Self {
        let min_cutoff = 1.5;
        let beta = 0.1;
        let d_cutoff = 1.0;
        let depth_min_cutoff = 3.0;
        let depth_beta = 0.02;
        let depth_d_cutoff = 2.0;

        Self {
            target_x: 0.0,
            target_y: 0.0,
            target_z: 80.0,
            median_x: MedianFilter3::new(),
            filter_x: OneEuroFilter::new(depth_min_cutoff, depth_beta, depth_d_cutoff),
            filter_y: OneEuroFilter::new(min_cutoff, beta, d_cutoff),
            filter_z: OneEuroFilter::new(min_cutoff, beta, d_cutoff),
            optimal_x: 355.0,
            optimal_y: -15.0,
            optimal_z: 80.0,

            current_error: 0.5,
            error_rate: 8.0, // ~125ms — fast color response

            center_threshold: 15.0,
            far_threshold: 80.0,

            current_depth_error: 0.5,
            current_depth_vibrancy: 1.0,
            depth_error_rate: 15.0,
            depth_vibrancy_rate: 10.0,
            depth_good_range: 100.0,
            depth_dim_range: 300.0,
            min_vibrancy: 0.1,

            velocity_y: 0.0,
            velocity_z: 0.0,
            last_update: Instant::now(),

            frame_count: 0,
            has_position: false,
        }
    }

    pub fn update_position(&mut self, x: f64, y: f64, z: f64) {
        if self.has_position {
            let elapsed = self.last_update.elapsed().as_secs_f64();
            if elapsed > 0.01 {
                self.velocity_y = (y - self.target_y) / elapsed;
                self.velocity_z = (z - self.target_z) / elapsed;
            }
        }
        self.target_x = x;
        self.target_y = y;
        self.target_z = z;
        self.last_update = Instant::now();
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

        let time_since = self.last_update.elapsed().as_secs_f64().min(0.12);
        let predicted_y = self.target_y + self.velocity_y * time_since;
        let predicted_z = self.target_z + self.velocity_z * time_since;

        let smooth_y = self.filter_y.filter(predicted_y, dt);
        let smooth_z = self.filter_z.filter(predicted_z, dt);

        // Depth: median kills shot noise, then One Euro smooths adaptively
        let median_x = self.median_x.filter(self.target_x);
        let smooth_x = self.filter_x.filter(median_x, dt);
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
