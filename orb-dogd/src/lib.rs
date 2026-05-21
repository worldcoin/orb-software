//! sync, drop-on-overflow statsd client.
//!
//! [`MetricEmitter::emit`] queues a metric onto a bounded channel and returns
//! immediately. A background worker (see [`DogstatsdClient`]) drains the
//! channel and performs the UDP send to the metric collector.
#![forbid(unsafe_code)]

mod dd;
pub use dd::DogstatsdClient;
pub use dogstatsd::DogstatsdError;

#[cfg(any(test, feature = "testing"))]
pub mod test;

/// Failure of [`MetricEmitter::emit`].
///
/// Wraps the underlying channel's send error so the choice of channel
/// implementation stays an internal detail of this crate.
#[derive(thiserror::Error, Debug)]
pub enum MetricError {
    /// Channel at capacity. The rejected [`Metric`] is returned so
    /// callers may retry or drop.
    #[error("channel is at capacity")]
    ChannelFull(Metric),
    /// Consumer is closed. Every subsequent `emit` will also
    /// fail with this variant.
    #[error("channel is closed")]
    ChannelClosed(Metric),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Metric {
    /// Counter delta; Used by:
    /// [`MetricEmitter::count`] and [`MetricEmitter::incr`]
    Count {
        stat: String,
        val: i64,
        tags: Vec<String>,
    },
    /// Last point-in-time value;
    /// Used by: [`MetricEmitter::gauge`]
    Gauge {
        stat: String,
        val: f64,
        tags: Vec<String>,
    },
    /// Metric aggregated by the **local Datadog agent**.
    /// Percentiles are per-host only;
    /// Used by [`MetricEmitter::hist`].
    Histogram {
        stat: String,
        val: f64,
        tags: Vec<String>,
    },
    /// Raw metrics are aggregated by the **Datadog's backend**.
    /// Supports global percentiles and post-hoc tag splits.
    /// Used by [`MetricEmitter::dist`].
    Distribution {
        stat: String,
        val: f64,
        tags: Vec<String>,
    },
}

/// Sink for [`Metric`]s.
pub trait MetricEmitter: Send + Sync + 'static {
    /// Queue `metric` for publishing
    fn emit(&self, metric: Metric) -> Result<(), MetricError>;

    fn count(&self, stat: &str, val: i64, tags: &[&str]) -> Result<(), MetricError> {
        self.emit(Metric::Count {
            stat: stat.to_owned(),
            val,
            tags: tags.iter().map(|t| (*t).to_owned()).collect(),
        })
    }

    fn gauge(&self, stat: &str, val: f64, tags: &[&str]) -> Result<(), MetricError> {
        self.emit(Metric::Gauge {
            stat: stat.to_owned(),
            val,
            tags: tags.iter().map(|t| (*t).to_owned()).collect(),
        })
    }

    fn incr(&self, stat: &str, tags: &[&str]) -> Result<(), MetricError> {
        self.emit(Metric::Count {
            stat: stat.to_owned(),
            val: 1,
            tags: tags.iter().map(|t| (*t).to_owned()).collect(),
        })
    }

    fn hist(&self, stat: &str, val: f64, tags: &[&str]) -> Result<(), MetricError> {
        self.emit(Metric::Histogram {
            stat: stat.to_owned(),
            val,
            tags: tags.iter().map(|t| (*t).to_owned()).collect(),
        })
    }

    fn dist(&self, stat: &str, val: f64, tags: &[&str]) -> Result<(), MetricError> {
        self.emit(Metric::Distribution {
            stat: stat.to_owned(),
            val,
            tags: tags.iter().map(|t| (*t).to_owned()).collect(),
        })
    }
}
