use dogstatsd::Client;
use flume::{Sender, TrySendError};
use std::{fs, path::Path, thread, time::Duration};
use tracing::{error, info, warn};

use super::{MetricEmitter, MetricError};

pub struct DogstatsdClient {
    tx: Sender<Metric>,
}

const DOGSTATSD_SOCKET_PATH: &str = "/run/datadog/dsd.socket";
const DOGSTATSD_BACKOFF: Duration = Duration::from_secs(3);

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

impl Default for DogstatsdClient {
    fn default() -> Self {
        Self::new()
    }
}

impl DogstatsdClient {
    /// Connect to the local statsd collector.
    ///
    /// Fails if the underlying socket cannot be bound.
    pub fn new() -> Self {
        let (tx, rx) = flume::bounded(512);

        thread::spawn(move || {
            let client = loop {
                let err_msg =
                    if fs::exists(Path::new(DOGSTATSD_SOCKET_PATH)).unwrap_or(false) {
                        info!("datadog-agent socket found, using it for IPC");

                        let opts = dogstatsd::OptionsBuilder::new()
                            .socket_path(Some(DOGSTATSD_SOCKET_PATH.to_string()))
                            .build();

                        match Client::new(opts) {
                            Ok(client) => break client,
                            Err(e) => format!("failed to create DD client {e}"),
                        }
                    } else {
                        format!("{DOGSTATSD_SOCKET_PATH} not found")
                    };

                error!(
                    "{err_msg}. waiting {}s and trying again",
                    DOGSTATSD_BACKOFF.as_secs()
                );

                thread::sleep(DOGSTATSD_BACKOFF);
            };

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

        Self { tx }
    }

    fn emit(&self, metric: Metric) -> Result<(), MetricError> {
        self.tx.try_send(metric).map_err(|e| match e {
            TrySendError::Full(_) => eyre::eyre!("transport channel is full: {e:#?}"),
            TrySendError::Disconnected(_) => eyre::eyre!("worker has died: {e:#?}"),
        })?;
        Ok(())
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
        self.emit(metric)
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
        self.emit(metric)
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
        self.emit(metric)
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
        self.emit(metric)
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
        self.emit(metric)
    }
}
