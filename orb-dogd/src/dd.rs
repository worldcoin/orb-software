use dogstatsd::{Client, DogstatsdError, Options};
use flume::Sender;
use std::thread;
use tracing::warn;

use super::{MetricEmitter, MetricError};

pub struct DogstatsdClient {
    tx: Sender<Metric>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Metric {
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

impl DogstatsdClient {
    /// Connect to the local statsd collector.
    ///
    /// Fails if the underlying socket cannot be bound.
    pub fn new() -> Result<Self, DogstatsdError> {
        let (tx, rx) = flume::unbounded();

        let datadog_options = Options::default();
        let client = Client::new(datadog_options)?;

        thread::spawn(move || {
            while let Ok(metric) = rx.recv() {
                match metric {
                    Metric::Count { stat, val, tags } => {
                        if let Err(e) = client.count(stat, val, tags) {
                            warn!("emitting metric failed with: {e}");
                        }
                    }
                    Metric::Gauge { stat, val, tags } => {
                        if let Err(e) = client.gauge(stat, val.to_string(), tags) {
                            warn!("emitting metric failed with: {e}");
                        }
                    }
                    Metric::Histogram { stat, val, tags } => {
                        if let Err(e) = client.histogram(stat, val.to_string(), tags) {
                            warn!("emitting metric failed with: {e}");
                        }
                    }
                    Metric::Distribution { stat, val, tags } => {
                        if let Err(e) = client.distribution(stat, val.to_string(), tags)
                        {
                            warn!("emitting metric failed with: {e}");
                        }
                    }
                }
            }
        });

        Ok(Self { tx })
    }
}

impl MetricEmitter for DogstatsdClient {
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
        self.tx
            .send(metric)
            .map_err(|_| eyre::eyre!("metrics worker has died"))?;

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
        self.tx
            .send(metric)
            .map_err(|_| eyre::eyre!("metrics worker has died"))?;

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
        self.tx
            .send(metric)
            .map_err(|_| eyre::eyre!("metrics worker has died"))?;

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
        self.tx
            .send(metric)
            .map_err(|_| eyre::eyre!("metrics worker has died"))?;

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
        self.tx
            .send(metric)
            .map_err(|_| eyre::eyre!("metrics worker has died"))?;

        Ok(())
    }
}
