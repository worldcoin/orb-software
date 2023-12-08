//! Universal [PID controller](https://en.wikipedia.org/wiki/PID_controller).
//!
//! This implementation has minimal constant memory footprint of only 80
//! bytes. It implements Proportional, Integral, Derivative terms, and also an
//! adjustable low-pass filter for Derivative term.
//!
//! If one of the terms is not needed, it's preferable not to set it at all, as
//! the respective computation is completely skipped by default. I.e. don't call
//! `pid.with_integral(0.0)`.
//!
//! # PID Terms
//!
//! *Proportional* - the main term, which should be adjusted the first. Controls
//! the basic speed of the process.
//!
//! *Integral* - adds inertia and acceleration to the process. Tends to start
//! slowly at the beginning, and overshoot at the end.
//!
//! *Derivative* - damping the response. It's able to cancel the overshooting
//! effect. Prone to input noises.
//!
//! *Filter* - cancels high-frequency noise from the *Derivative* term input.
//! Defined in terms of seconds. Usually set to `dt * N`, where `dt` is the
//! average interval between the PID controller updates, and `N` is an empiric
//! constant in order of `20`.
//!
//! # Variable Names
//!
//! `setpoint` (SP) is the desired value, the PID controller should eventually
//! reach to.
//!
//! `process` (process value, PV) is the measured value of the external process
//! controllable by the PID controller.
//!
//! `control` (control variable) is the output of the PID controller, which
//! should be applied to the external process. Manual negation (`-control`) may
//! be needed depending on the process, because negative gain parameters are not
//! supported.
//!
//! # Examples
//!
//! ```
//! use pid::{InstantTimer, Pid, Timer};
//!
//! let mut timer = InstantTimer::default();
//! let mut pid = Pid::default()
//!     .with_proportional(2.0)
//!     .with_integral(0.5)
//!     .with_derivative(1.0)
//!     .with_filter(0.01);
//! for process in 0..10 {
//!     /* obtain `process` value from the sensor */
//!     let dt = timer.get_dt().unwrap_or(0.0);
//!     let control = pid.advance(1.0, process as f64, dt);
//!     /* apply `control` to the actuator */
//! }
//! ```
//!
//! Use the default `InstantTimer` for tracking time intervals, or provide the
//! raw `dt` value directly.

pub mod derivative;
pub mod integral;

mod timer;

pub use self::timer::{ConstDelta, InstantTimer, Timer};

use self::{derivative::LowPassFilter, integral::RiemannSum};

/// Universal PID controller.
///
/// See [the module-level documentation](self) for details.
#[derive(Clone, Default, Debug)]
pub struct Pid {
    proportional: Option<f64>,
    integral: Option<f64>,
    derivative: Option<f64>,
    rc: f64,
    filter: LowPassFilter,
    sum: RiemannSum,
}

impl Pid {
    /// Sets the proportional gain. This method takes self by value and allows
    /// chaining.
    #[must_use]
    pub fn with_proportional(mut self, proportional: f64) -> Self {
        self.set_proportional(proportional);
        self
    }

    /// Sets the integral gain. This method takes self by value and allows
    /// chaining.
    #[must_use]
    pub fn with_integral(mut self, integral: f64) -> Self {
        self.set_integral(integral);
        self
    }

    /// Sets the derivative gain. This method takes self by value and allows
    /// chaining.
    #[must_use]
    pub fn with_derivative(mut self, derivative: f64) -> Self {
        self.set_derivative(derivative);
        self
    }

    /// Sets the time constant for the low-pass filter. This method takes self
    /// by value and allows chaining.
    ///
    /// # Panics
    ///
    /// If `rc` is negative.
    #[must_use]
    pub fn with_filter(mut self, rc: f64) -> Self {
        self.set_filter(rc);
        self
    }

    /// Sets the proportional gain. This method takes self by mutable reference
    /// and allows chaining.
    pub fn set_proportional(&mut self, proportional: f64) -> &mut Self {
        self.proportional = Some(proportional);
        self
    }

    /// Sets the integral gain. This method takes self by mutable reference and
    /// allows chaining.
    pub fn set_integral(&mut self, integral: f64) -> &mut Self {
        self.integral = Some(integral);
        self
    }

    /// Sets the derivative gain. This method takes self by mutable reference
    /// and allows chaining.
    pub fn set_derivative(&mut self, derivative: f64) -> &mut Self {
        self.derivative = Some(derivative);
        self
    }

    /// Sets the time constant for the low-pass filter. This method takes self
    /// by mutable reference and allows chaining.
    ///
    /// # Panics
    ///
    /// If `rc` is negative.
    pub fn set_filter(&mut self, rc: f64) -> &mut Self {
        assert!(rc >= 0.0, "filter time constant must not be negative");
        self.rc = rc;
        self
    }

    /// Resets the accumulated state.
    pub fn reset(&mut self) {
        self.filter.reset();
        self.sum.reset();
    }

    /// Advances the PID loop with new `setpoint` and `process` variables, and
    /// the time passed since last invocation `dt`. Returns calculated control
    /// variable.
    pub fn advance(&mut self, setpoint: f64, process: f64, dt: f64) -> f64 {
        let error = setpoint - process;
        let mut control = 0.0;
        if let Some(proportional) = self.proportional {
            control += proportional * error;
        }
        if let Some(integral) = self.integral {
            control += integral * self.sum.add(error, dt);
        }
        if let Some(derivative) = self.derivative {
            control +=
                derivative * self.filter.add_slope(error, dt, self.rc).unwrap_or(0.0);
        }
        control
    }
}
