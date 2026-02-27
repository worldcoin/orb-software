use crate::engine::{Animation, AnimationState, CenterFrame, RingFrame, Transition};
use orb_rgb::Argb;
use std::time::Instant;
use std::{any::Any, f64::consts::PI};

/// LED dimming value for Diamond hardware.
const DIMMING: Option<u8> = Some(31);

// Depth boundaries in mm (from orb-core distance_range_in).
const DEPTH_CLOSE_LIMIT: f64 = 200.0;
const DEPTH_FAR_LIMIT: f64 = 510.0;
const HYSTERESIS_MM: f64 = 10.0;

// Sweet spot: fill fraction above which we consider the user centered.
const SWEET_SPOT_FILL: f64 = 0.98;

// Sweet spot requires depth within a tighter band around optimal (355mm).
const SWEET_SPOT_DEPTH_CLOSE: f64 = 280.0;
const SWEET_SPOT_DEPTH_FAR: f64 = 430.0;

// Centering guidance only activates within this tighter depth band.
// Outside this but still InRange, keep spinning the beacon.
const GUIDANCE_DEPTH_CLOSE: f64 = 250.0;
const GUIDANCE_DEPTH_FAR: f64 = 460.0;

// Too-far: spinning white arc.
const SPIN_PERIOD: f64 = 2.0;
const SPIN_ARC_WIDTH: f64 = PI / 3.0; // 60° arc
const SPIN_EDGE_WIDTH: f64 = PI / 6.0; // 30° soft edge
const SPIN_MIN_BRIGHTNESS: f64 = 0.08; // dim background

// Center ring dim white during centering guidance (brightness 0-1).
const DIM_CENTER_BRIGHTNESS: f64 = 0.15;

// -----------------------------------------------------------------------
// Depth state machine with hysteresis
// -----------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DepthState {
    TooClose,
    InRange,
    TooFar,
}

impl DepthState {
    /// Asymmetric hysteresis: entering TooClose/TooFar triggers right
    /// at the limit, but leaving requires clearing by HYSTERESIS_MM.
    /// Red feels responsive on approach, sticky once active.
    fn update(self, depth_mm: f64) -> Self {
        match self {
            Self::TooClose => {
                if depth_mm > DEPTH_CLOSE_LIMIT + HYSTERESIS_MM {
                    if depth_mm > DEPTH_FAR_LIMIT {
                        Self::TooFar
                    } else {
                        Self::InRange
                    }
                } else {
                    self
                }
            }
            Self::InRange => {
                if depth_mm < DEPTH_CLOSE_LIMIT {
                    Self::TooClose
                } else if depth_mm > DEPTH_FAR_LIMIT {
                    Self::TooFar
                } else {
                    self
                }
            }
            Self::TooFar => {
                if depth_mm < DEPTH_FAR_LIMIT - HYSTERESIS_MM {
                    if depth_mm < DEPTH_CLOSE_LIMIT {
                        Self::TooClose
                    } else {
                        Self::InRange
                    }
                } else {
                    self
                }
            }
        }
    }
}

// -----------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------

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

/// Exponential moving average (dt-aware).
fn ema(current: f64, target: f64, rate: f64, dt: f64) -> f64 {
    current + (target - current) * (1.0 - (-rate * dt).exp())
}

// -----------------------------------------------------------------------
// Median filter — kills single-frame shot noise before One Euro
// -----------------------------------------------------------------------

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
        if (a >= b && a <= c) || (a <= b && a >= c) {
            a
        } else if (b >= a && b <= c) || (b <= a && b >= c) {
            b
        } else {
            c
        }
    }
}

// -----------------------------------------------------------------------
// One Euro Filter — adaptive low-pass with velocity-dependent cutoff
// -----------------------------------------------------------------------

fn smoothing_factor(dt: f64, cutoff: f64) -> f64 {
    let tau = 1.0 / (2.0 * PI * cutoff);
    dt / (dt + tau)
}

struct OneEuroFilter {
    x_prev: f64,
    dx_prev: f64,
    min_cutoff: f64,
    beta: f64,
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

        let dx = (x - self.x_prev) / dt;
        let alpha_d = smoothing_factor(dt, self.d_cutoff);
        let dx_hat = alpha_d * dx + (1.0 - alpha_d) * self.dx_prev;
        let cutoff = self.min_cutoff + self.beta * dx_hat.abs();
        let alpha = smoothing_factor(dt, cutoff);
        let x_hat = alpha * x + (1.0 - alpha) * self.x_prev;

        self.x_prev = x_hat;
        self.dx_prev = dx_hat;

        x_hat
    }
}

// -----------------------------------------------------------------------
// Outer ring: 4-state position feedback
//
//  TooClose  → solid red
//  TooFar    → breathing white (slow sine)
//  InRange   → cyan directional fill (Face ID arc style)
//  SweetSpot → solid white
// -----------------------------------------------------------------------

pub struct PositionFeedback<const N: usize> {
    target_x: f64,
    target_y: f64,
    target_z: f64,
    median_x: MedianFilter3,
    filter_y: OneEuroFilter,
    filter_z: OneEuroFilter,

    optimal_y: f64,
    optimal_z: f64,

    current_fill: f64,
    fill_origin: f64,
    error_rate: f64,
    origin_rate: f64,

    center_threshold: f64,
    far_threshold: f64,

    depth_state: DepthState,
    state_phase: f64,

    /// Authoritative in-range signal from orb-core's distance sensor.
    /// When Some(false), overrides depth_state to TooClose/TooFar.
    distance_in_range: Option<bool>,

    velocity_y: f64,
    velocity_z: f64,
    last_update: Instant,

    frame_count: u32,
    has_position: bool,
}

impl<const N: usize> PositionFeedback<N> {
    pub fn new(_color: Argb) -> Self {
        let min_cutoff = 1.5;
        let beta = 0.1;
        let d_cutoff = 1.0;
        Self {
            target_x: 0.0,
            target_y: 0.0,
            target_z: 80.0,
            median_x: MedianFilter3::new(),
            filter_y: OneEuroFilter::new(min_cutoff, beta, d_cutoff),
            filter_z: OneEuroFilter::new(min_cutoff, beta, d_cutoff),

            optimal_y: -15.0,
            optimal_z: 80.0,

            current_fill: 0.0,
            fill_origin: 0.0,
            error_rate: 8.0,
            origin_rate: 15.0,

            center_threshold: 15.0,
            far_threshold: 80.0,

            depth_state: DepthState::TooFar,
            state_phase: 0.0,
            distance_in_range: None,

            velocity_y: 0.0,
            velocity_z: 0.0,
            last_update: Instant::now(),

            frame_count: 0,
            has_position: false,
        }
    }

    /// Authoritative in-range signal from orb-core's distance (ToF)
    /// sensor. When false, forces out-of-range regardless of face
    /// engine depth estimate.
    pub fn update_in_range(&mut self, in_range: bool) {
        self.distance_in_range = Some(in_range);
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
    fn animate(
        &mut self,
        frame: &mut RingFrame<N>,
        dt: f64,
        idle: bool,
    ) -> AnimationState {
        if idle || !self.has_position {
            return AnimationState::Running;
        }

        self.frame_count += 1;
        self.state_phase += dt;

        // Dead reckoning for ML latency compensation.
        let time_since = self.last_update.elapsed().as_secs_f64().min(0.12);
        let predicted_y = self.target_y + self.velocity_y * time_since;
        let predicted_z = self.target_z + self.velocity_z * time_since;

        // Filter pipeline: median → One Euro.
        let smooth_y = self.filter_y.filter(predicted_y, dt);
        let smooth_z = self.filter_z.filter(predicted_z, dt);
        let median_x = self.median_x.filter(self.target_x);

        // Depth state: prefer orb-core's distance sensor (authoritative).
        // Fall back to face engine depth via median filter.
        self.depth_state = if let Some(false) = self.distance_in_range {
            // orb-core says out of range — use face engine depth
            // to decide TooClose vs TooFar.
            if median_x < (DEPTH_CLOSE_LIMIT + DEPTH_FAR_LIMIT) / 2.0 {
                DepthState::TooClose
            } else {
                DepthState::TooFar
            }
        } else if let Some(true) = self.distance_in_range {
            DepthState::InRange
        } else {
            self.depth_state.update(median_x)
        };

        match self.depth_state {
            DepthState::TooClose => {
                // Solid red — both rings.
                let color = Argb(DIMMING, 255, 0, 0);
                for led in frame.iter_mut() {
                    *led = color;
                }
            }
            DepthState::TooFar => {
                // Spinning white arc.
                let spin_angle = (self.state_phase / SPIN_PERIOD) * 2.0 * PI;
                for (i, led) in frame.iter_mut().enumerate() {
                    let led_angle = (i as f64 / N as f64) * 2.0 * PI;
                    let mut dist = (led_angle - spin_angle).abs();
                    if dist > PI {
                        dist = 2.0 * PI - dist;
                    }
                    let fade =
                        ((SPIN_ARC_WIDTH - dist) / SPIN_EDGE_WIDTH).clamp(0.0, 1.0);
                    let brightness =
                        SPIN_MIN_BRIGHTNESS + (1.0 - SPIN_MIN_BRIGHTNESS) * fade;
                    let v = (255.0 * brightness).round() as u8;
                    *led = Argb(DIMMING, v, v, v);
                }
            }
            DepthState::InRange => {
                let in_guidance_zone =
                    median_x >= GUIDANCE_DEPTH_CLOSE && median_x <= GUIDANCE_DEPTH_FAR;

                if !in_guidance_zone {
                    // Near edge of range — keep spinning beacon.
                    let spin_angle = (self.state_phase / SPIN_PERIOD) * 2.0 * PI;
                    for (i, led) in frame.iter_mut().enumerate() {
                        let led_angle = (i as f64 / N as f64) * 2.0 * PI;
                        let mut dist = (led_angle - spin_angle).abs();
                        if dist > PI {
                            dist = 2.0 * PI - dist;
                        }
                        let fade =
                            ((SPIN_ARC_WIDTH - dist) / SPIN_EDGE_WIDTH).clamp(0.0, 1.0);
                        let brightness =
                            SPIN_MIN_BRIGHTNESS + (1.0 - SPIN_MIN_BRIGHTNESS) * fade;
                        let v = (255.0 * brightness).round() as u8;
                        *led = Argb(DIMMING, v, v, v);
                    }
                } else {
                    // Y/Z centering: offset from optimal.
                    let dy = smooth_y - self.optimal_y;
                    let dz = smooth_z - self.optimal_z;
                    let distance = (dy * dy + dz * dz).sqrt();

                    if distance > 5.0 {
                        let a = dy.atan2(-dz);
                        let target_origin = if a < 0.0 { a + 2.0 * PI } else { a };
                        self.fill_origin = angle_ema(
                            self.fill_origin,
                            target_origin,
                            self.origin_rate,
                            dt,
                        );
                    }

                    let target_fill = (1.0
                        - ((distance - self.center_threshold)
                            / (self.far_threshold - self.center_threshold))
                            .clamp(0.0, 1.0))
                    .clamp(0.0, 1.0);
                    self.current_fill =
                        ema(self.current_fill, target_fill, self.error_rate, dt);

                    let depth_in_sweet = median_x >= SWEET_SPOT_DEPTH_CLOSE
                        && median_x <= SWEET_SPOT_DEPTH_FAR;

                    if self.current_fill >= SWEET_SPOT_FILL && depth_in_sweet {
                        let color = Argb(DIMMING, 0, 255, 0);
                        for led in frame.iter_mut() {
                            *led = color;
                        }
                    } else {
                        let shaped_fill =
                            self.current_fill * self.current_fill * self.current_fill;
                        let fill_half_angle = shaped_fill * PI;
                        let edge_width = 2.0 * PI / N as f64 * 3.0;

                        for (i, led) in frame.iter_mut().enumerate() {
                            let led_angle = (i as f64 / N as f64) * 2.0 * PI;
                            let mut dist_from_origin =
                                (led_angle - self.fill_origin).abs();
                            if dist_from_origin > PI {
                                dist_from_origin = 2.0 * PI - dist_from_origin;
                            }

                            let fade = ((fill_half_angle - dist_from_origin)
                                / edge_width)
                                .clamp(0.0, 1.0);
                            let v = (255.0 * fade).round() as u8;
                            *led = Argb(DIMMING, v, v, v);
                        }
                    }
                }
            }
        }

        if self.frame_count % 180 == 0 {
            tracing::info!(
                "ring_fb: state={:?} fill={:.2} \
                 origin={:.0}° depth={:.0}mm",
                self.depth_state,
                self.current_fill,
                self.fill_origin.to_degrees(),
                median_x,
            );
        }

        AnimationState::Running
    }

    fn stop(&mut self, _transition: Transition) -> eyre::Result<()> {
        Ok(())
    }
}

// -----------------------------------------------------------------------
// Center ring: mirrors outer ring depth states
//
//  TooClose  → solid red
//  TooFar    → breathing white (matches outer ring)
//  InRange   → dim white (keeps "eye" alive)
//  SweetSpot → solid white
// -----------------------------------------------------------------------

pub struct PositionFeedbackCenter<const N: usize> {
    target_x: f64,
    target_y: f64,
    target_z: f64,
    median_x: MedianFilter3,
    filter_y: OneEuroFilter,
    filter_z: OneEuroFilter,

    optimal_y: f64,
    optimal_z: f64,

    current_fill: f64,
    error_rate: f64,

    center_threshold: f64,
    far_threshold: f64,

    depth_state: DepthState,
    state_phase: f64,
    distance_in_range: Option<bool>,

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
        Self {
            target_x: 0.0,
            target_y: 0.0,
            target_z: 80.0,
            median_x: MedianFilter3::new(),
            filter_y: OneEuroFilter::new(min_cutoff, beta, d_cutoff),
            filter_z: OneEuroFilter::new(min_cutoff, beta, d_cutoff),

            optimal_y: -15.0,
            optimal_z: 80.0,

            current_fill: 0.0,
            error_rate: 8.0,

            center_threshold: 15.0,
            far_threshold: 80.0,

            depth_state: DepthState::TooFar,
            state_phase: 0.0,
            distance_in_range: None,

            velocity_y: 0.0,
            velocity_z: 0.0,
            last_update: Instant::now(),

            frame_count: 0,
            has_position: false,
        }
    }

    pub fn update_in_range(&mut self, in_range: bool) {
        self.distance_in_range = Some(in_range);
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

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
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
        self.state_phase += dt;

        let time_since = self.last_update.elapsed().as_secs_f64().min(0.12);
        let predicted_y = self.target_y + self.velocity_y * time_since;
        let predicted_z = self.target_z + self.velocity_z * time_since;

        let smooth_y = self.filter_y.filter(predicted_y, dt);
        let smooth_z = self.filter_z.filter(predicted_z, dt);
        let median_x = self.median_x.filter(self.target_x);

        self.depth_state = if let Some(false) = self.distance_in_range {
            if median_x < (DEPTH_CLOSE_LIMIT + DEPTH_FAR_LIMIT) / 2.0 {
                DepthState::TooClose
            } else {
                DepthState::TooFar
            }
        } else if let Some(true) = self.distance_in_range {
            DepthState::InRange
        } else {
            self.depth_state.update(median_x)
        };

        // Y/Z centering for sweet spot detection.
        let dy = smooth_y - self.optimal_y;
        let dz = smooth_z - self.optimal_z;
        let distance = (dy * dy + dz * dz).sqrt();
        let target_fill = (1.0
            - ((distance - self.center_threshold)
                / (self.far_threshold - self.center_threshold))
                .clamp(0.0, 1.0))
        .clamp(0.0, 1.0);
        self.current_fill = ema(self.current_fill, target_fill, self.error_rate, dt);

        match self.depth_state {
            DepthState::TooClose => {
                let color = Argb(DIMMING, 255, 0, 0);
                for led in frame.iter_mut() {
                    *led = color;
                }
            }
            DepthState::TooFar => {
                // Spinning white arc (matches outer ring).
                let spin_angle = (self.state_phase / SPIN_PERIOD) * 2.0 * PI;
                for (i, led) in frame.iter_mut().enumerate() {
                    let led_angle = (i as f64 / N as f64) * 2.0 * PI;
                    let mut dist = (led_angle - spin_angle).abs();
                    if dist > PI {
                        dist = 2.0 * PI - dist;
                    }
                    let fade =
                        ((SPIN_ARC_WIDTH - dist) / SPIN_EDGE_WIDTH).clamp(0.0, 1.0);
                    let brightness =
                        SPIN_MIN_BRIGHTNESS + (1.0 - SPIN_MIN_BRIGHTNESS) * fade;
                    let v = (255.0 * brightness).round() as u8;
                    *led = Argb(DIMMING, v, v, v);
                }
            }
            DepthState::InRange => {
                let in_guidance_zone =
                    median_x >= GUIDANCE_DEPTH_CLOSE && median_x <= GUIDANCE_DEPTH_FAR;

                if !in_guidance_zone {
                    // Near edge of range — keep spinning.
                    let spin_angle = (self.state_phase / SPIN_PERIOD) * 2.0 * PI;
                    for (i, led) in frame.iter_mut().enumerate() {
                        let led_angle = (i as f64 / N as f64) * 2.0 * PI;
                        let mut dist = (led_angle - spin_angle).abs();
                        if dist > PI {
                            dist = 2.0 * PI - dist;
                        }
                        let fade =
                            ((SPIN_ARC_WIDTH - dist) / SPIN_EDGE_WIDTH).clamp(0.0, 1.0);
                        let brightness =
                            SPIN_MIN_BRIGHTNESS + (1.0 - SPIN_MIN_BRIGHTNESS) * fade;
                        let v = (255.0 * brightness).round() as u8;
                        *led = Argb(DIMMING, v, v, v);
                    }
                } else {
                    let depth_in_sweet = median_x >= SWEET_SPOT_DEPTH_CLOSE
                        && median_x <= SWEET_SPOT_DEPTH_FAR;
                    let color =
                        if self.current_fill >= SWEET_SPOT_FILL && depth_in_sweet {
                            Argb(DIMMING, 0, 255, 0)
                        } else {
                            let v = (255.0 * DIM_CENTER_BRIGHTNESS).round() as u8;
                            Argb(DIMMING, v, v, v)
                        };
                    for led in frame.iter_mut() {
                        *led = color;
                    }
                }
            }
        }

        if self.frame_count % 180 == 0 {
            tracing::info!(
                "center_fb: state={:?} fill={:.2} \
                 depth={:.0}mm",
                self.depth_state,
                self.current_fill,
                median_x,
            );
        }

        AnimationState::Running
    }

    fn stop(&mut self, _transition: Transition) -> eyre::Result<()> {
        Ok(())
    }
}
