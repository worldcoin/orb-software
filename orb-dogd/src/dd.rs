use super::{MetricEmitter, MetricError};
use dogstatsd::Client;
use flume::RecvError;
use flume::Sender;
use flume::TrySendError;
use std::thread;
use std::time::Instant;
use std::{fs, path::Path, time::Duration};
use tracing::{info, warn};

const DOGSTATSD_SOCKET_PATH: &str = "/run/datadog/dsd.socket";
const DOGSTATSD_BACKOFF: Duration = Duration::from_secs(10);
const DEFAULT_QUEUE_SIZE: usize = 4096;
const DEFAULT_MAX_EMIT_PER_TICK: usize = 25;
const DEFAULT_TICK: Duration = Duration::from_millis(50);

#[derive(Clone, Debug)]
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
    /// Milliseconds latency value; emits Datadog's `|ms` type.
    /// Used by [`MetricEmitter::timing`].
    Timing {
        stat: String,
        val: i64,
        tags: Vec<String>,
    },
}

impl Default for DogstatsdClient {
    fn default() -> Self {
        Self::new(DEFAULT_QUEUE_SIZE, DEFAULT_MAX_EMIT_PER_TICK, DEFAULT_TICK)
    }
}

impl DogstatsdClient {
    /// Connect to the local statsd collector.
    ///
    /// Fails if the underlying socket cannot be bound.
    pub fn new(
        queue_size: usize,
        max_emit_per_tick: usize,
        tick_duration: Duration,
    ) -> Self {
        let start = Instant::now();
        let (tx, rx) = flume::bounded(queue_size);

        thread::spawn(move || {
            let client = loop {
                let err_msg =
                    if fs::exists(Path::new(DOGSTATSD_SOCKET_PATH)).unwrap_or(false) {
                        info!(
                            "datadog-agent socket found, using it for IPC. took {}s",
                            start.elapsed().as_secs()
                        );

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

                warn!(
                    "{err_msg}. waiting {}s and trying again",
                    DOGSTATSD_BACKOFF.as_secs()
                );

                thread::sleep(DOGSTATSD_BACKOFF);
            };

            let mut current_tick = Instant::now();
            let mut count = 0;

            let mut send = |metric| {
                use Metric::*;

                match metric {
                    Count { stat, val, tags } => {
                        if let Err(e) = client.count(stat, val, tags) {
                            warn!("emitting metric failed with: {e}");
                        }
                    }

                    Gauge { stat, val, tags } => {
                        if let Err(e) = client.gauge(stat, val.to_string(), tags) {
                            warn!("emitting metric failed with: {e}");
                        }
                    }

                    Histogram { stat, val, tags } => {
                        if let Err(e) = client.histogram(stat, val.to_string(), tags) {
                            warn!("emitting metric failed with: {e}");
                        }
                    }

                    Distribution { stat, val, tags } => {
                        if let Err(e) = client.distribution(stat, val.to_string(), tags)
                        {
                            warn!("emitting metric failed with: {e}");
                        }
                    }

                    Timing { stat, val, tags } => {
                        if let Err(e) = client.timing(stat, val, tags) {
                            warn!("emitting metric failed with: {e}");
                        }
                    }
                }

                count += 1;

                if current_tick.elapsed() >= tick_duration || count >= max_emit_per_tick
                {
                    let sleep = tick_duration.saturating_sub(current_tick.elapsed());
                    if sleep > Duration::ZERO {
                        thread::sleep(sleep);
                    }

                    count = 0;
                    current_tick = Instant::now();
                }
            };

            info!(
                "starting dogd internal loop. queued messages: {}/{queue_size}",
                rx.len()
            );

            loop {
                let msg = match rx.recv() {
                    Err(RecvError::Disconnected) => {
                        warn!("main datadog channel disconnected, all clients were dropped, exiting thread!");
                        break;
                    }

                    Ok(msg) => msg,
                };

                send(msg);

                while let Ok(msg) = rx.try_recv() {
                    send(msg);
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
        self.emit(metric)
    }
}
