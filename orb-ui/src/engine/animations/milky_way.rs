use crate::engine::Animation;
use crate::engine::{AnimationState, RingFrame};
use orb_rgb::Argb;
use std::any::Any;
use std::f64::consts::PI;

pub const DIAMOND_DEFAULT_BACKGROUND_COLOR: Argb = Argb(Some(10), 25, 20, 10);

/// Idle / not animated ring = all LEDs in one color
/// by default, all off
pub struct MilkyWay<const N: usize> {
    pub(crate) background_color: Argb,
    phase: f64,
    stopping: bool,
    stopping_period: f64,
    frame: RingFrame<N>,
}

impl<const N: usize> MilkyWay<N> {
    /// Create idle ring
    #[must_use]
    pub fn new(background: Option<Argb>) -> Self {
        // generate initial randomized frame
        let frame: [Argb; N] =
            [background.unwrap_or(DIAMOND_DEFAULT_BACKGROUND_COLOR); N];
        for led in frame {
            let sign = if rand::random::<i8>() % 2 == 0 { 1 } else { -1 } as i8;
            Argb(
                led.0,
                (led.1 as i8 + (rand::random::<i8>() % 10) * sign)
                    .clamp(led.1 as i8 - 20_i8, led.1 as i8 + 20_i8)
                    as u8,
                (led.2 as i8 + (rand::random::<i8>() % 10) * sign)
                    .clamp(led.2 as i8 - 15_i8, led.2 as i8 + 15_i8)
                    as u8,
                (led.3 as i8 + (rand::random::<i8>() % 10) * sign)
                    .clamp(0, led.3 as i8 + 10_i8) as u8,
            );
        }
        Self {
            background_color: background.unwrap_or(DIAMOND_DEFAULT_BACKGROUND_COLOR),
            phase: 0.0,
            stopping: false,
            stopping_period: 1.5,
            frame,
        }
    }
}

impl<const N: usize> Default for MilkyWay<N> {
    fn default() -> Self {
        let frame: [Argb; N] = [DIAMOND_DEFAULT_BACKGROUND_COLOR; N];
        for led in frame {
            let sign = if rand::random::<i8>() % 2 == 0 { 1 } else { -1 } as i8;
            Argb(
                led.0,
                (led.1 as i8 + (rand::random::<i8>() % 10) * sign)
                    .clamp(led.1 as i8 - 20_i8, led.1 as i8 + 20_i8)
                    as u8,
                (led.2 as i8 + (rand::random::<i8>() % 10) * sign)
                    .clamp(led.2 as i8 - 15_i8, led.2 as i8 + 15_i8)
                    as u8,
                (led.3 as i8 + (rand::random::<i8>() % 10) * sign)
                    .clamp(0, led.3 as i8 + 10_i8) as u8,
            );
        }
        Self {
            background_color: DIAMOND_DEFAULT_BACKGROUND_COLOR,
            phase: 0.0,
            stopping: false,
            stopping_period: 1.5,
            frame,
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
            let mut sign = 1;
            let mut index = 0;
            for led in &mut self.frame {
                if index == 0 {
                    index = 3;
                    sign = if rand::random::<i8>() % 2 == 0 { 1 } else { -1 } as i8;
                } else {
                    index -= 1;
                }

                let red = (led.1 as i8 + (rand::random::<i8>() % 3) * sign).clamp(
                    self.background_color.1 as i8 - 20_i8,
                    self.background_color.1 as i8 + 20_i8,
                ) as u8;
                let green = (led.2 as i8 + (rand::random::<i8>() % 3) * sign).clamp(
                    self.background_color.2 as i8 - 15_i8,
                    self.background_color.2 as i8 + 15_i8,
                ) as u8;
                let blue = (led.3 as i8 + (rand::random::<i8>() % 2) * sign).clamp(
                    self.background_color.3 as i8 - 10_i8,
                    self.background_color.3 as i8 + 10_i8,
                ) as u8;
                *led = Argb(led.0, red, green, blue);
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
