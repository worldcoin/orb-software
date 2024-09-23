use crate::engine::Animation;
use crate::engine::{AnimationState, RingFrame};
use orb_rgb::Argb;
use std::any::Any;
use std::f64::consts::PI;

/// Idle / not animated ring = all LEDs in one color
/// by default, all off
pub struct MilkyWay<const N: usize> {
    phase: f64,
    stopping: bool,
    stopping_period: f64,
    frame: RingFrame<N>,
    config: MilkyWayConfig,
}

struct MilkyWayConfig {
    background: Argb,
    fade_delta: i8,
    red_delta: i8,
    green_delta: i8,
    blue_delta: i8,
    red_min_max: (u8, u8),
    green_min_max: (u8, u8),
    blue_min_max: (u8, u8),
}

const MILKY_WAY_DEFAULT: MilkyWayConfig = MilkyWayConfig {
    background: Argb(Some(10), 30, 20, 5),
    fade_delta: 2,
    red_delta: 20,
    green_delta: 15,
    blue_delta: 5,
    red_min_max: (10, 40),
    green_min_max: (1, 40),
    blue_min_max: (1, 10),
};

fn generate_random(frame: &mut [Argb], config: &MilkyWayConfig) {
    let new_color = |config: &MilkyWayConfig| {
        let sign = if rand::random::<i8>() % 2 == 0 { 1 } else { -1 } as i8;
        Argb(
            config.background.0,
            (config.background.1 as i8
                + (rand::random::<i8>() % config.red_delta) * sign)
                .clamp(config.red_min_max.0 as i8, config.red_min_max.1 as i8)
                as u8,
            (config.background.2 as i8
                + (rand::random::<i8>() % config.green_delta) * sign)
                .clamp(config.green_min_max.0 as i8, config.green_min_max.1 as i8)
                as u8,
            (config.background.3 as i8
                + (rand::random::<i8>() % config.blue_delta) * sign)
                .clamp(config.blue_min_max.0 as i8, config.blue_min_max.1 as i8)
                as u8,
        )
    };

    let mut c = new_color(config);
    for (i, led) in frame.iter_mut().enumerate() {
        if i % 2 == 0 {
            c = new_color(config);
        }
        *led = c;
    }
}

impl<const N: usize> MilkyWay<N> {
    /// Create idle ring
    #[must_use]
    pub fn new(background: Option<Argb>) -> Self {
        let mut config = MILKY_WAY_DEFAULT;
        config.background = background.unwrap_or(config.background);

        // generate initial randomized frame
        let mut frame: [Argb; N] =
            [background.unwrap_or(MILKY_WAY_DEFAULT.background); N];
        generate_random(&mut frame, &config);

        Self {
            phase: 0.0,
            stopping: false,
            stopping_period: 1.5,
            frame,
            config,
        }
    }
}

impl<const N: usize> Default for MilkyWay<N> {
    fn default() -> Self {
        let mut frame: [Argb; N] = [MILKY_WAY_DEFAULT.background; N];
        generate_random(&mut frame, &MILKY_WAY_DEFAULT);
        Self {
            phase: 0.0,
            stopping: false,
            stopping_period: 1.5,
            frame,
            config: MILKY_WAY_DEFAULT,
        }
    }
}

impl<const N: usize> Animation for MilkyWay<N> {
    type Frame = RingFrame<N>;

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    #[allow(clippy::cast_precision_loss)]
    fn animate(
        &mut self,
        frame: &mut RingFrame<N>,
        dt: f64,
        idle: bool,
    ) -> AnimationState {
        if !self.stopping {
            let mut sign;
            let mut color = self.frame[0];
            for (i, led) in &mut self.frame.iter_mut().enumerate() {
                if i % 2 == 0 {
                    sign = if rand::random::<i8>() % 2 == 0 { 1 } else { -1 } as i8;
                    color = Argb(
                        led.0,
                        (led.1 as i8
                            + (rand::random::<i8>() % self.config.fade_delta) * sign)
                            .clamp(
                                self.config.red_min_max.0 as i8,
                                self.config.red_min_max.1 as i8,
                            ) as u8,
                        (led.2 as i8
                            + (rand::random::<i8>() % self.config.fade_delta) * sign)
                            .clamp(
                                self.config.green_min_max.0 as i8,
                                self.config.green_min_max.1 as i8,
                            ) as u8,
                        (led.3 as i8
                            + (rand::random::<i8>() % self.config.fade_delta) * sign)
                            .clamp(
                                self.config.blue_min_max.0 as i8,
                                self.config.blue_min_max.1 as i8,
                            ) as u8,
                    );
                }

                *led = color;
            }

            if !idle {
                frame.copy_from_slice(&self.frame);
            }
        } else if self.phase < self.stopping_period {
            // apply sine wave to stop the animation smoothly
            let factor = (self.phase / self.stopping_period * PI / 2.0).cos();
            for (led, background_led) in frame.iter_mut().zip(&self.frame) {
                *led = Argb(
                    background_led.0,
                    (background_led.1 as f64 * factor).round() as u8,
                    (background_led.2 as f64 * factor).round() as u8,
                    (background_led.3 as f64 * factor).round() as u8,
                );
            }
            self.phase += dt;
        } else {
            return AnimationState::Finished;
        }

        AnimationState::Running
    }

    fn stop(&mut self) {
        self.stopping = true;
        self.phase = 0.0;
    }

    fn transition_from(&mut self, _superseded: &dyn Any) {}
}
