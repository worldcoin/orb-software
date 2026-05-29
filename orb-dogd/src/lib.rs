//! Standardized statsd client interface.
//!
//! [`MetricEmitter`] is an abstraction trait bounding the API;
//! [`DogstatsdClient`] is used for the default  implementation.
//! Test implementations are gated behind the `testing` feature.
//!
//! # Fork safety
//!
//! [`DogstatsdClient`] owns a background worker thread that does the actual
//! socket I/O. `fork(2)` only clones the calling thread, so the worker does not
//! exist in the child. To stay usable across a fork the client records the PID
//! that owns its worker and, on the first emit after a fork, notices the PID
//! changed and transparently respawns the worker in the child. A process-global
//! `static DATADOG: Lazy<DogstatsdClient>` therefore keeps working in children
//! whether it is first touched before or after the fork. See `tests/fork.rs`.
#![forbid(unsafe_code)]

mod dd;
pub use dd::DogstatsdClient;
pub use dogstatsd::DogstatsdError;

#[cfg(any(test, feature = "testing"))]
pub mod test;

/// Empty tag set.
pub const NO_TAGS: [&str; 0] = [];

/// Failure from a [`MetricEmitter`] method.
#[derive(thiserror::Error, Debug)]
#[error(transparent)]
pub struct MetricError(#[from] pub eyre::Report);

/// Statsd-style metric sink.
pub trait MetricEmitter: Send + Sync + 'static {
    /// Counter delta
    fn count<S, I>(&self, stat: S, val: i64, tags: I) -> Result<(), MetricError>
    where
        S: Into<String>,
        I: IntoIterator<Item: Into<String>>;

    /// Single increment of a counter
    fn incr<S, I>(&self, stat: S, tags: I) -> Result<(), MetricError>
    where
        S: Into<String>,
        I: IntoIterator<Item: Into<String>>;

    /// Last point-in-time value;
    fn gauge<S, I>(&self, stat: S, val: f64, tags: I) -> Result<(), MetricError>
    where
        S: Into<String>,
        I: IntoIterator<Item: Into<String>>;

    /// Metric aggregated by the **local Datadog agent**.
    fn hist<S, I>(&self, stat: S, val: f64, tags: I) -> Result<(), MetricError>
    where
        S: Into<String>,
        I: IntoIterator<Item: Into<String>>;

    /// Metrics are aggregated by the **Datadog's backend**.
    fn dist<S, I>(&self, stat: S, val: f64, tags: I) -> Result<(), MetricError>
    where
        S: Into<String>,
        I: IntoIterator<Item: Into<String>>;

    /// Latency in milliseconds; emits Datadog's `|ms` type, which the
    /// backend treats as a timing-flavored histogram.
    fn timing<S, I>(&self, stat: S, val: i64, tags: I) -> Result<(), MetricError>
    where
        S: Into<String>,
        I: IntoIterator<Item: Into<String>>;
}
