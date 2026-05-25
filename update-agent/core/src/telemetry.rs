use once_cell::sync::Lazy;
use orb_dogd::{DogstatsdClient, MetricError};

pub use orb_dogd::MetricEmitter;

pub static DATADOG: Lazy<DogstatsdClient> = Lazy::new(|| {
    DogstatsdClient::new().expect("failed to construct DogstatsdClient")
});

/// A trait for logging errors instead of propagating the error with `?`.
pub trait LogOnError {
    /// Logs an error message to the default logger at the `Error` level.
    fn or_log(&self);
}

impl<T> LogOnError for Result<T, MetricError> {
    fn or_log(&self) {
        if let Err(e) = self {
            tracing::error!("metric emit failed: {e:#?}");
        }
    }
}
