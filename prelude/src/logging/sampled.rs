use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct LogSite {
    level: LogLevel,
    file: &'static str,
    line: u32,
    column: u32,
}

impl LogSite {
    #[doc(hidden)]
    pub fn new(level: LogLevel, file: &'static str, line: u32, column: u32) -> Self {
        Self {
            level,
            file,
            line,
            column,
        }
    }
}

#[doc(hidden)]
#[macro_export]
macro_rules! __sampled_log {
    ($logger:expr, $level:expr, $trace:ident, $($arg:tt)+) => {{
        let site = $crate::logging::sampled::LogSite::new($level, file!(), line!(), column!());

        if let Some(repeated) = $logger.hit(site) {
            if repeated == 0 {
                tracing::$trace!($($arg)+);
            } else {
                tracing::$trace!("{} (repeated {} times)", format_args!($($arg)+), repeated);
            }
        }
    }};
}

#[doc(hidden)]
#[macro_export]
macro_rules! __sampled_info {
    ($logger:expr, $($arg:tt)+) => {
        $crate::__sampled_log!($logger, $crate::logging::sampled::LogLevel::Info, info, $($arg)+)
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __sampled_warn {
    ($logger:expr, $($arg:tt)+) => {
        $crate::__sampled_log!($logger, $crate::logging::sampled::LogLevel::Warn, warn, $($arg)+)
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __sampled_error {
    ($logger:expr, $($arg:tt)+) => {
        $crate::__sampled_log!($logger, $crate::logging::sampled::LogLevel::Error, error, $($arg)+)
    };
}

pub use crate::{
    __sampled_error as error, __sampled_info as info, __sampled_warn as warn,
};

/// Samples logs within a timeframe to avoid excessive logging.
///
/// Logs are sampled per call site and level. The call site is identified by
/// the macro invocation's file, line, and column.
///
/// # Example
///
/// ```
/// use std::time::Duration;
///
/// use prelude::logging::sampled::{self, LogSampler};
///
/// let mut logger = LogSampler::new(Duration::from_secs(1));
///
/// sampled::info!(logger, "waiting for {}", "capture");
/// sampled::warn!(logger, "retrying operation");
/// sampled::error!(logger, "operation failed: {}", "timeout");
/// ```
pub struct LogSampler {
    throttle: Duration,
    seen: HashMap<LogSite, (Instant, u64)>,
}

impl LogSampler {
    pub fn new(throttle: Duration) -> Self {
        Self {
            throttle,
            seen: HashMap::new(),
        }
    }

    #[doc(hidden)]
    pub fn hit(&mut self, site: LogSite) -> Option<u64> {
        let now = Instant::now();

        let Some((last, suppressed)) = self.seen.get_mut(&site) else {
            self.seen.insert(site, (now, 0));
            return Some(0);
        };

        if now.duration_since(*last) < self.throttle {
            *suppressed += 1;
            return None;
        }

        let repeated = *suppressed;
        *last = now;
        *suppressed = 0;

        Some(repeated)
    }
}
