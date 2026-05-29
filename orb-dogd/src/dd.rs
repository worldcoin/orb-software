use dogstatsd::Client;
use flume::Sender;
use flume::TrySendError;
use rustix::process::{getpid, Pid};
use std::sync::Mutex;
use std::thread;
use std::{fs, path::Path, time::Duration};
use tracing::warn;
use tracing::{error, info};

use super::{MetricEmitter, MetricError};

pub struct DogstatsdClient {
    socket_path: String,
    worker: Mutex<Worker>,
}

/// The current worker thread's channel, and the PID that owns it. `fork(2)`
/// clones only the calling thread, so a worker spawned before a fork does not
/// exist in the child. Tracking the owning PID lets [`DogstatsdClient`] notice
/// it has been forked and respawn the worker in the child.
struct Worker {
    owner_pid: Pid,
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
        Self::new()
    }
}

/// Spawn the worker thread that owns the socket connection and drains `rx`,
/// returning the channel that feeds it. Lives outside `impl` so it can be
/// called both at construction and when respawning after a fork.
fn spawn_worker(socket_path: String) -> Sender<Metric> {
    let (tx, rx) = flume::bounded(512);

    thread::spawn(move || {
        let client = loop {
            let err_msg = if fs::exists(Path::new(&socket_path)).unwrap_or(false) {
                info!("datadog-agent socket found, using it for IPC");

                let opts = dogstatsd::OptionsBuilder::new()
                    .socket_path(Some(socket_path.clone()))
                    .build();

                match Client::new(opts) {
                    Ok(client) => break client,
                    Err(e) => format!("failed to create DD client {e}"),
                }
            } else {
                format!("{socket_path} not found")
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
                    if let Err(e) = client.distribution(stat, val.to_string(), tags) {
                        warn!("emitting metric failed with: {e}");
                    }
                }
                Metric::Timing { stat, val, tags } => {
                    if let Err(e) = client.timing(stat, val, tags) {
                        warn!("emitting metric failed with: {e}");
                    }
                }
            }
        }
    });

    tx
}

impl DogstatsdClient {
    /// Connect to the local statsd collector at the default socket path.
    pub fn new() -> Self {
        Self::with_socket_path(DOGSTATSD_SOCKET_PATH)
    }

    /// Connect to the statsd collector listening at `socket_path`.
    ///
    /// The connection is established lazily on a dedicated worker thread: if
    /// the socket is not present yet the worker backs off and retries until it
    /// appears.
    ///
    /// The client is fork-safe: it records the PID that owns the worker, and on
    /// the first emit after a `fork(2)` it notices the PID changed and respawns
    /// the worker in the child (the parent's worker thread does not survive the
    /// fork). See the crate-level docs.
    pub fn with_socket_path(socket_path: impl Into<String>) -> Self {
        let socket_path = socket_path.into();
        let tx = spawn_worker(socket_path.clone());

        Self {
            socket_path,
            worker: Mutex::new(Worker {
                owner_pid: getpid(),
                tx,
            }),
        }
    }

    /// Return the sender for this process's worker, respawning it first if we
    /// have been forked since the worker was last created.
    ///
    /// The lock is held only for a PID compare and a cheap `Sender` clone, not
    /// for the send. Respawning only ever happens in a child (the parent's PID
    /// always matches), so the parent never holds this lock when a fork copies
    /// it, and the child cannot inherit it locked.
    fn sender(&self) -> Sender<Metric> {
        let pid = getpid();
        let mut worker = self.worker.lock().expect("dogd worker mutex poisoned");
        if worker.owner_pid != pid {
            warn!("dogd: detected fork; respawning metrics worker in new process");
            worker.tx = spawn_worker(self.socket_path.clone());
            worker.owner_pid = pid;
        }

        worker.tx.clone()
    }

    fn emit(&self, metric: Metric) -> Result<(), MetricError> {
        self.sender().try_send(metric).map_err(|e| match e {
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
