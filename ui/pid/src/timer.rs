use std::{fmt::Debug, mem, time::Instant};

/// Timer for PID controller.
pub trait Timer: Clone + Default + Debug + Send + Sync + Sized {
    /// Returns time delta since previous invocation, or `None` if this is the
    /// first invocation.
    fn get_dt(&mut self) -> Option<f64>;

    /// Resets the timer state.
    fn reset(&mut self);
}

/// [`Instant`] based timer for PID controller.
#[allow(clippy::module_name_repetitions)] // `timer` module is not a part of public API
#[derive(Clone, Default, Debug)]
pub struct InstantTimer {
    last_time: Option<Instant>,
}

/// Timer that always produces constant time deltas.
#[derive(Clone, Default, Debug)]
pub struct ConstDelta {
    dt: f64,
    running: bool,
}

impl Timer for InstantTimer {
    fn get_dt(&mut self) -> Option<f64> {
        let now = Instant::now();
        self.last_time
            .replace(now)
            .map(|last_time| (now - last_time).as_secs_f64())
    }

    fn reset(&mut self) {
        self.last_time = None;
    }
}

impl Timer for ConstDelta {
    fn get_dt(&mut self) -> Option<f64> {
        mem::replace(&mut self.running, true).then_some(self.dt)
    }

    fn reset(&mut self) {
        self.running = false;
    }
}

impl From<f64> for ConstDelta {
    fn from(dt: f64) -> Self {
        Self {
            dt,
            ..Self::default()
        }
    }
}
