//! Standardized statsd client interface.
//!
//! [`MetricEmitter`] is an abstraction trait bounding the API;
//! [`DogstatsdClient`] is used for the default  implementation.
//! Test implementations are gated behind the `testing` feature.
#![forbid(unsafe_code)]

mod dd;
pub use dd::DogstatsdClient;

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
