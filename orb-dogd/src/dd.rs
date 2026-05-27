use flume::Sender;
use flume::TrySendError;
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixDatagram;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::{fs, path::Path, time::Duration};
use tracing::warn;
use tracing::{error, info};

use super::{MetricEmitter, MetricError};

pub struct DogstatsdClient {
    tx: Sender<Metric>,
    connection: Arc<Connection>,
}

const DOGSTATSD_SOCKET_PATH: &str = "/run/datadog/dsd.socket";
const DOGSTATSD_BACKOFF: Duration = Duration::from_secs(3);
const CHANNEL_CAPACITY: usize = 512;

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

/// Holds the connected dsd socket once the background retry loop has
/// reached the daemon. `wait_for_connection` blocks on the condvar until
/// the bg thread publishes a connected socket here.
struct Connection {
    socket: Mutex<Option<UnixDatagram>>,
    cv: Condvar,
}

impl Connection {
    fn new() -> Self {
        Self {
            socket: Mutex::new(None),
            cv: Condvar::new(),
        }
    }

    fn publish(&self, socket: UnixDatagram) {
        let mut guard = self.socket.lock().expect("connection mutex poisoned");
        *guard = Some(socket);
        self.cv.notify_all();
    }
}

impl Default for DogstatsdClient {
    fn default() -> Self {
        Self::new()
    }
}

impl DogstatsdClient {
    /// Connect to the local statsd collector at `/run/datadog/dsd.socket`.
    ///
    /// Returns immediately. A background thread opens the socket with retry
    /// and then drains the metric channel. The channel is bounded; once it
    /// fills (typically when the daemon is unreachable for an extended
    /// period), new emissions are dropped via `try_send` rather than
    /// blocking the caller.
    pub fn new() -> Self {
        let (tx, rx) = flume::bounded(CHANNEL_CAPACITY);
        let connection = Arc::new(Connection::new());

        let conn = Arc::clone(&connection);
        thread::spawn(move || {
            let socket = loop {
                let err_msg =
                    if fs::exists(Path::new(DOGSTATSD_SOCKET_PATH)).unwrap_or(false) {
                        match try_connect(DOGSTATSD_SOCKET_PATH) {
                            Ok(socket) => {
                                info!("datadog-agent socket found, using it for IPC");
                                break socket;
                            }
                            Err(e) => format!("failed to connect to DD daemon: {e}"),
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

            // Publish a clone of the connected socket so external callers
            // (e.g. orb-core's ProcessInitializer) can dup the FD and pass
            // it to sandboxed subprocesses, which cannot connect from
            // inside a separate network namespace.
            match socket.try_clone() {
                Ok(shared) => conn.publish(shared),
                Err(e) => error!("failed to clone dsd socket for sharing: {e}"),
            }

            emit_loop(rx, socket);
        });

        Self { tx, connection }
    }

    /// Build a client that emits over an already-connected Unix datagram
    /// socket inherited from a parent process. Used by sandboxed
    /// subprocesses (network namespace) that cannot `connect()` to the dsd
    /// socket themselves. No retry; emit failures are logged.
    pub fn from_unix_datagram(socket: UnixDatagram) -> Self {
        let (tx, rx) = flume::bounded(CHANNEL_CAPACITY);
        let connection = Arc::new(Connection::new());

        thread::spawn(move || {
            emit_loop(rx, socket);
        });

        Self { tx, connection }
    }

    /// Block until the bg thread has connected to the dsd daemon, then
    /// return a dup'd `OwnedFd` of the connected socket. The returned FD
    /// can be passed across `fork()` so a subprocess in a separate network
    /// namespace can still emit metrics via [`Self::from_unix_datagram`].
    ///
    /// Returns `None` if the timeout elapses without a connection.
    pub fn wait_for_connection(&self, timeout: Duration) -> Option<OwnedFd> {
        let guard = self
            .connection
            .socket
            .lock()
            .expect("connection mutex poisoned");
        let (guard, result) = self
            .connection
            .cv
            .wait_timeout_while(guard, timeout, |s| s.is_none())
            .expect("connection mutex poisoned");
        if result.timed_out() {
            return None;
        }
        let socket = guard.as_ref()?;
        match socket.try_clone() {
            Ok(cloned) => Some(OwnedFd::from(cloned)),
            Err(e) => {
                error!("failed to clone dsd socket for export: {e}");
                None
            }
        }
    }

    fn emit(&self, metric: Metric) -> Result<(), MetricError> {
        self.tx.try_send(metric).map_err(|e| match e {
            TrySendError::Full(_) => eyre::eyre!("transport channel is full: {e:#?}"),
            TrySendError::Disconnected(_) => eyre::eyre!("worker has died: {e:#?}"),
        })?;
        Ok(())
    }
}

fn try_connect(path: &str) -> std::io::Result<UnixDatagram> {
    let socket = UnixDatagram::unbound()?;
    socket.connect(path)?;
    Ok(socket)
}

fn emit_loop(rx: flume::Receiver<Metric>, socket: UnixDatagram) {
    while let Ok(metric) = rx.recv() {
        let payload = format_metric(&metric);
        if let Err(e) = socket.send(payload.as_bytes()) {
            warn!("emitting metric failed with: {e}");
        }
    }
}

fn format_metric(metric: &Metric) -> String {
    match metric {
        Metric::Count { stat, val, tags } => format_payload(stat, val, "c", tags),
        Metric::Gauge { stat, val, tags } => format_payload(stat, val, "g", tags),
        Metric::Histogram { stat, val, tags } => format_payload(stat, val, "h", tags),
        Metric::Distribution { stat, val, tags } => format_payload(stat, val, "d", tags),
        Metric::Timing { stat, val, tags } => format_payload(stat, val, "ms", tags),
    }
}

// Tags are joined as-is. Tags containing `,` or `|` will be misparsed by
// the daemon; this matches the prior behavior with the `dogstatsd-rs`
// crate, which also does no escaping. Callers are responsible for keeping
// tag strings free of these characters.
fn format_payload<V: std::fmt::Display>(
    stat: &str,
    val: V,
    ty: &str,
    tags: &[String],
) -> String {
    if tags.is_empty() {
        format!("{stat}:{val}|{ty}")
    } else {
        format!("{stat}:{val}|{ty}|#{}", tags.join(","))
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
