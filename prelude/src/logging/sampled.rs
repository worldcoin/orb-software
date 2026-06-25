use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

#[doc(hidden)]
pub use tracing;

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
    ($logger:expr, $level:expr, $trace:ident, $message:literal) => {{
        let site = $crate::logging::sampled::LogSite::new($level, file!(), line!(), column!());

        if let Some(repeated) = $logger.hit(site) {
            if repeated == 0 {
                $crate::logging::sampled::tracing::$trace!($message);
            } else {
                $crate::logging::sampled::tracing::$trace!(
                    "{} (repeated {} times)",
                    format_args!($message),
                    repeated
                );
            }
        }
    }};
    ($logger:expr, $level:expr, $trace:ident, $message:literal, $($arg:tt)+) => {{
        let site = $crate::logging::sampled::LogSite::new($level, file!(), line!(), column!());

        if let Some(repeated) = $logger.hit(site) {
            if repeated == 0 {
                $crate::logging::sampled::tracing::$trace!($message, $($arg)+);
            } else {
                $crate::logging::sampled::tracing::$trace!(
                    "{} (repeated {} times)",
                    format_args!($message, $($arg)+),
                    repeated
                );
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
/// The logging macros intentionally accept format-style log messages only. They
/// do not support structured `tracing` fields such as `target:` or `field = %value`.
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

#[cfg(test)]
mod tests {
    use std::{
        io::{self, Write},
        sync::{Arc, Mutex},
        thread,
        time::Duration,
    };

    use super::LogSampler;
    use crate::logging::sampled;

    #[derive(Clone, Default)]
    struct SharedWriter {
        output: Arc<Mutex<Vec<u8>>>,
    }

    impl SharedWriter {
        fn contents(&self) -> String {
            String::from_utf8(self.output.lock().unwrap().clone()).unwrap()
        }
    }

    impl Write for SharedWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.output.lock().unwrap().write(buf)
        }

        fn flush(&mut self) -> io::Result<()> {
            self.output.lock().unwrap().flush()
        }
    }

    fn log_implicit_capture(logger: &mut LogSampler, err: &str) {
        sampled::warn!(logger, "failed {err}");
    }

    #[test]
    fn repeated_log_formats_implicit_capture() {
        // Arrange
        let writer = SharedWriter::default();
        let writer_for_subscriber = writer.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .without_time()
            .with_writer(move || writer_for_subscriber.clone())
            .finish();

        let mut logger = LogSampler::new(Duration::from_millis(1));

        // Act
        tracing::subscriber::with_default(subscriber, || {
            log_implicit_capture(&mut logger, "first");
            log_implicit_capture(&mut logger, "suppressed");
            thread::sleep(Duration::from_millis(2));
            log_implicit_capture(&mut logger, "summary");
        });

        // Assert
        let output = writer.contents();

        assert!(
            output.contains("failed first"),
            "expected first log message in output: {output}"
        );
        assert!(
            !output.contains("failed suppressed"),
            "suppressed message should not be logged: {output}"
        );
        assert!(
            output.contains("failed summary (repeated 1 times)"),
            "expected repeated summary to format implicit capture: {output}"
        );
        assert!(
            !output.contains("failed {err}"),
            "implicit capture should not be logged as a raw format string: {output}"
        );
    }
}
