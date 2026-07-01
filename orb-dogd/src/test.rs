//! Test purposed [`MetricEmitter`] implementations.
//!
//! Gated behind the `testing` feature; intended for `dev-dependencies` of
//! crates that want to assert on emitted metrics.

use std::sync::{Arc, Mutex};

use super::dd::Metric;
use super::{MetricEmitter, MetricError};

/// Counts emitted metrics. Clones share the same record buffer, so a test
/// can keep a handle for assertions while passing another clone into the
/// code under test.
#[derive(Clone, Default)]
pub struct MetricRecorder {
    records: Arc<Mutex<Vec<Metric>>>,
}

impl MetricRecorder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of metrics emitted so far.
    pub fn len(&self) -> usize {
        self.records.lock().expect("mutex poisoned").len()
    }

    /// Whether any metric has been emitted yet.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[cfg(test)]
    pub(crate) fn records(&self) -> Vec<Metric> {
        self.records.lock().expect("mutex poisoned").clone()
    }
}

impl MetricEmitter for MetricRecorder {
    fn count<S, I>(&self, stat: S, val: i64, tags: I) -> Result<(), MetricError>
    where
        S: Into<String>,
        I: IntoIterator<Item: Into<String>>,
    {
        let metric = Metric::Count {
            stat: stat.into(),
            val,
            tags: tags.into_iter().map(Into::into).collect(),
        };
        self.records.lock().expect("mutex poisoned").push(metric);
        Ok(())
    }

    fn incr<S, I>(&self, stat: S, tags: I) -> Result<(), MetricError>
    where
        S: Into<String>,
        I: IntoIterator<Item: Into<String>>,
    {
        let metric = Metric::Count {
            stat: stat.into(),
            val: 1,
            tags: tags.into_iter().map(Into::into).collect(),
        };
        self.records.lock().expect("mutex poisoned").push(metric);
        Ok(())
    }

    fn gauge<S, I>(&self, stat: S, val: f64, tags: I) -> Result<(), MetricError>
    where
        S: Into<String>,
        I: IntoIterator<Item: Into<String>>,
    {
        let metric = Metric::Gauge {
            stat: stat.into(),
            val,
            tags: tags.into_iter().map(Into::into).collect(),
        };
        self.records.lock().expect("mutex poisoned").push(metric);
        Ok(())
    }

    fn hist<S, I>(&self, stat: S, val: f64, tags: I) -> Result<(), MetricError>
    where
        S: Into<String>,
        I: IntoIterator<Item: Into<String>>,
    {
        let metric = Metric::Histogram {
            stat: stat.into(),
            val,
            tags: tags.into_iter().map(Into::into).collect(),
        };
        self.records.lock().expect("mutex poisoned").push(metric);
        Ok(())
    }

    fn dist<S, I>(&self, stat: S, val: f64, tags: I) -> Result<(), MetricError>
    where
        S: Into<String>,
        I: IntoIterator<Item: Into<String>>,
    {
        let metric = Metric::Distribution {
            stat: stat.into(),
            val,
            tags: tags.into_iter().map(Into::into).collect(),
        };
        self.records.lock().expect("mutex poisoned").push(metric);
        Ok(())
    }

    fn timing<S, I>(&self, stat: S, val: i64, tags: I) -> Result<(), MetricError>
    where
        S: Into<String>,
        I: IntoIterator<Item: Into<String>>,
    {
        let metric = Metric::Timing {
            stat: stat.into(),
            val,
            tags: tags.into_iter().map(Into::into).collect(),
        };
        self.records.lock().expect("mutex poisoned").push(metric);
        Ok(())
    }
}

/// [`MetricEmitter`] that drops every metric on the floor. Useful when test
/// code requires some emitter but assertions don't care what was emitted.
pub struct MetricSinkhole;

impl MetricEmitter for MetricSinkhole {
    fn count<S, I>(&self, _: S, _: i64, _: I) -> Result<(), MetricError>
    where
        S: Into<String>,
        I: IntoIterator<Item: Into<String>>,
    {
        Ok(())
    }

    fn incr<S, I>(&self, _: S, _: I) -> Result<(), MetricError>
    where
        S: Into<String>,
        I: IntoIterator<Item: Into<String>>,
    {
        Ok(())
    }

    fn gauge<S, I>(&self, _: S, _: f64, _: I) -> Result<(), MetricError>
    where
        S: Into<String>,
        I: IntoIterator<Item: Into<String>>,
    {
        Ok(())
    }

    fn hist<S, I>(&self, _: S, _: f64, _: I) -> Result<(), MetricError>
    where
        S: Into<String>,
        I: IntoIterator<Item: Into<String>>,
    {
        Ok(())
    }

    fn dist<S, I>(&self, _: S, _: f64, _: I) -> Result<(), MetricError>
    where
        S: Into<String>,
        I: IntoIterator<Item: Into<String>>,
    {
        Ok(())
    }

    fn timing<S, I>(&self, _: S, _: i64, _: I) -> Result<(), MetricError>
    where
        S: Into<String>,
        I: IntoIterator<Item: Into<String>>,
    {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::NO_TAGS;

    use super::*;

    #[test]
    fn count_records_count_variant() {
        let r = MetricRecorder::new();
        r.count("topic", 3, NO_TAGS).unwrap();
        assert_eq!(
            r.records(),
            vec![Metric::Count {
                stat: "topic".to_owned(),
                val: 3,
                tags: vec![],
            }],
        );
    }

    /// Many threads emit concurrently through a shared recorder, and every
    /// emission lands in the shared record buffer.
    #[test]
    fn recorder_collects_emissions_across_threads() {
        const THREADS: usize = 8;
        const PER_THREAD: usize = 8;

        let recorder = MetricRecorder::new();
        let emitter = Arc::new(recorder.clone());

        std::thread::scope(|s| {
            for _ in 0..THREADS {
                s.spawn(|| {
                    for _ in 0..PER_THREAD {
                        emitter.count("topic", 1, ["k:v"]).unwrap();
                    }
                });
            }
        });

        assert_eq!(recorder.len(), THREADS * PER_THREAD);
    }
}
