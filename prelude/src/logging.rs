use std::{
    collections::HashMap,
    panic::Location,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum LogLevel {
    Info,
    Warn,
    Error,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct LogSite {
    level: LogLevel,
    file: &'static str,
    line: u32,
    column: u32,
}

impl LogSite {
    fn new(level: LogLevel, location: &'static Location<'static>) -> Self {
        Self {
            level,
            file: location.file(),
            line: location.line(),
            column: location.column(),
        }
    }
}

pub struct ThrottledLogger {
    throttle: Duration,
    seen: HashMap<LogSite, (Instant, u64)>,
}

impl ThrottledLogger {
    pub fn new(throttle: Duration) -> Self {
        Self {
            throttle,
            seen: HashMap::new(),
        }
    }

    #[track_caller]
    pub fn info(&mut self, msg: &str) {
        let site = LogSite::new(LogLevel::Info, Location::caller());

        if let Some(repeated) = self.hit(site) {
            if repeated == 0 {
                tracing::info!("{msg}");
            } else {
                tracing::info!("{msg} (repeated {repeated} times)");
            }
        }
    }

    #[track_caller]
    pub fn warn(&mut self, msg: &str) {
        let site = LogSite::new(LogLevel::Warn, Location::caller());

        if let Some(repeated) = self.hit(site) {
            if repeated == 0 {
                tracing::warn!("{msg}");
            } else {
                tracing::warn!("{msg} (repeated {repeated} times)");
            }
        }
    }

    #[track_caller]
    pub fn error(&mut self, msg: &str) {
        let site = LogSite::new(LogLevel::Error, Location::caller());

        if let Some(repeated) = self.hit(site) {
            if repeated == 0 {
                tracing::error!("{msg}");
            } else {
                tracing::error!("{msg} (repeated {repeated} times)");
            }
        }
    }

    fn hit(&mut self, site: LogSite) -> Option<u64> {
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
