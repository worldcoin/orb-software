//! Test purposed [`MetricEmitter`] implementations.
//!
//! Gated behind the `testing` feature; intended for `dev-dependencies` of
//! crates that want to assert on emitted metrics.

use std::sync::{Arc, Mutex};

use super::{Metric, MetricEmitter, MetricError};

/// In-memory recorder for [`Metric`]s. Cheap to [`Clone`] — clones share the
/// same record buffer, so a test can keep a handle for assertions while
/// passing another clone into the code under test.
#[derive(Clone, Default)]
pub struct MetricRecorder {
    records: Arc<Mutex<Vec<Metric>>>,
}

impl MetricRecorder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Snapshot of every [`Metric`] emitted so far.
    pub fn records(&self) -> Vec<Metric> {
        self.records.lock().expect("mutex poisoned").clone()
    }
}

impl MetricEmitter for MetricRecorder {
    fn emit(&self, metric: Metric) -> Result<(), MetricError> {
        self.records.lock().expect("mutex poisoned").push(metric);
        Ok(())
    }
}

/// [`MetricEmitter`] that drops every metric on the floor. Useful when test
/// code requires some emitter but assertions don't care what was emitted.
pub struct MetricSinkhole;

impl MetricEmitter for MetricSinkhole {
    fn emit(&self, _: Metric) -> Result<(), MetricError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_records_count_variant() {
        let r = MetricRecorder::new();
        r.count("topic", 3, &["k:v"]).unwrap();
        assert_eq!(
            r.records(),
            vec![Metric::Count {
                stat: "topic".to_owned(),
                val: 3,
                tags: vec!["k:v".to_owned()],
            }],
        );
    }

    /// Validates the cross-thread contract of [`MetricEmitter`]:
    /// many threads emit concurrently through a shared `Arc<dyn MetricEmitter>`,
    /// and every emission lands in the shared record buffer.
    ///
    /// Compiling this test relies on `MetricEmitter: Send + Sync + 'static`
    /// and on [`MetricRecorder`]'s internal `Arc<Mutex<_>>` (so every clone
    /// observes the same buffer).
    #[test]
    fn recorder_collects_emissions_across_threads() {
        const THREADS: usize = 8;
        const PER_THREAD: usize = 8;

        let recorder = MetricRecorder::new();
        let emitter: Arc<dyn MetricEmitter> = Arc::new(recorder.clone());

        std::thread::scope(|s| {
            for _ in 0..THREADS {
                s.spawn(|| {
                    for _ in 0..PER_THREAD {
                        emitter.count("topic", 1, &[]).unwrap();
                    }
                });
            }
        });

        assert_eq!(recorder.records().len(), THREADS * PER_THREAD);
    }
}
