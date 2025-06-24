use crate::engine::{Animation, AnimationState};
use orb_rgb::Argb;
use std::any::Any;
use std::f64::consts::PI;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AlertError {
    #[error("Edge {} `activation_time` should be later than {}", _0, _1)]
    InitializationError(usize, f64),
}

pub struct SquarePulseEdge {
    activation_time: f64,
    smooth_duration: f64,
}

impl From<(f64, f64)> for SquarePulseEdge {
    fn from(value: (f64, f64)) -> Self {
        Self {
            activation_time: value.0,
            smooth_duration: value.1,
        }
    }
}

pub struct SquarePulseTrain(Vec<SquarePulseEdge>);

impl From<Vec<(f64, f64)>> for SquarePulseTrain {
    fn from(value: Vec<(f64, f64)>) -> Self {
        Self(value.into_iter().map(SquarePulseEdge::from).collect())
    }
}

/// # Alert
///
/// The `Alert` struct creates a flashing LED effect by turning the LEDs on and off
/// in a timed sequence. It uses a series of pulses (defined in a `SquarePulseTrain`)
/// to smoothly transition between off and a target color.
///
/// ## What It Does
///
/// - **Initial Delay:**
///   The alert can wait for a set time before starting the animation.
///
/// - **Pulses:**  
///   The animation is made up of pulses. Each pulse has two parts:
///   - A **rising edge** (even-indexed pulse) where the LED brightness increases from off
///     to the target color.
///   - A **falling edge** (odd-indexed pulse) where the brightness decreases from the target color back to off.
///
/// - **Timing:**
///   Each pulse is defined by:
///   - `activation_time`: When the pulse starts.
///   - `smooth_duration`: How long it takes to complete the fade-in or fade-out.
///
/// - **Animation End:**  
///   Once all pulses have been processed, the animation stops.
///
/// ## Example Timeline
///
/// ```rust
/// // Each tuple is (activation_time, smooth_duration)
/// let pulse_train = SquarePulseTrain::from(vec![(0.0, 0.5), (1.0, 0.5)]);
/// ```
/// assuming you use red as the target color:
///
/// - **Time 0.0s:**
///   The animation starts (after any initial delay). The first (rising) pulse begins:
///   the LED brightness starts at 0 and gradually increases toward red over 0.5 seconds.
///
/// - **Time 0.5s:**
///   The rising pulse is complete, and the LED shows full red.
///
/// - **Time 1.0s:**
///   The second (falling) pulse starts: the LED brightness begins to decrease from red toward off,
///   taking another 0.5 seconds.
///
/// - **Time 1.5s:**
///   The falling pulse is complete, the LED is off, and the animation finishes.
///
pub struct Alert<const N: usize> {
    current_edge: usize,
    target_color: Argb,
    /// Describes the wave-form.
    square_pulse_train: SquarePulseTrain,
    /// time in animation
    phase: f64,
    /// initial delay, in seconds, before starting the animation
    initial_delay: f64,
}

impl<const N: usize> Alert<N> {
    /// Creates a new [`Alert`].
    pub fn new(
        color: Argb,
        square_pulse_train: SquarePulseTrain,
    ) -> Result<Self, AlertError> {
        for (i, window) in square_pulse_train.0.windows(2).enumerate() {
            let previous = &window[0];
            let current = &window[1];
            let required_activation =
                previous.activation_time + previous.smooth_duration;
            if current.activation_time < required_activation {
                return Err(AlertError::InitializationError(
                    i + 1,
                    required_activation,
                ));
            }
        }
        Ok(Self {
            target_color: color,
            phase: 0.0,
            initial_delay: 0.0,
            square_pulse_train,
            current_edge: 0,
        })
    }

    #[allow(dead_code)]
    pub fn with_delay(mut self, delay: f64) -> Self {
        self.initial_delay = delay;
        self
    }
}

impl<const N: usize> Animation for Alert<N> {
    type Frame = [Argb; N];

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn animate(
        &mut self,
        frame: &mut [Argb; N],
        dt: f64,
        idle: bool,
    ) -> AnimationState {
        // initial delay
        if self.initial_delay > 0.0 {
            self.initial_delay -= dt;
            return AnimationState::Running;
        }

        let rising_edge = self.current_edge % 2 == 0;
        let current_edge = &self.square_pulse_train.0[self.current_edge];

        let color = if self.phase < current_edge.activation_time {
            // this can only happen if the first edge starts later than 0.0
            Argb::OFF
        } else if self.phase
            < current_edge.activation_time + current_edge.smooth_duration
        {
            let t = self.phase - current_edge.activation_time;
            let intensity = if rising_edge {
                0.5 * (1.0 - (PI / current_edge.smooth_duration * t).cos())
            } else {
                0.5 * (1.0 + (PI / current_edge.smooth_duration * t).cos())
            };
            self.target_color * intensity
        } else if rising_edge {
            self.target_color
        } else {
            Argb::OFF
        };

        if !idle {
            for led in frame {
                *led = color;
            }
        }

        if self.current_edge == self.square_pulse_train.0.len() - 1
            && self.phase > current_edge.activation_time + current_edge.smooth_duration
        {
            return AnimationState::Finished;
        }

        self.phase += dt;
        while self.current_edge + 1 < self.square_pulse_train.0.len()
            && self.phase
                > self.square_pulse_train.0[self.current_edge + 1].activation_time
        {
            self.current_edge += 1;
        }

        AnimationState::Running
    }
}
