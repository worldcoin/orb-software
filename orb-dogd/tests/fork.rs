//! Fork-safety of a process-global `Lazy<DogstatsdClient>`.
//!
//! [`DogstatsdClient`] does its socket I/O on a background worker thread.
//! `fork(2)` clones only the calling thread, so the worker does not exist in a
//! child. These tests pin down what that means in practice:
//!
//! * [`child_constructed_after_fork_delivers`] — the `Lazy` is first forced
//!   *after* the fork, so each child gets its own fresh worker and its metrics
//!   reach the collector, including when the parent is multithreaded at fork
//!   time.
//! * [`pre_fork_client_reconnects_in_child`] — the `Lazy` is forced *before*
//!   the fork. The worker does not survive into the child, so the child's first
//!   emit detects the PID change and respawns the worker; both parent and child
//!   deliver.
//!
//! The real client hardcodes `/run/datadog/dsd.socket`; these tests redirect it
//! to a temp socket via [`DogstatsdClient::with_socket_path`] and stand up a
//! `UnixDatagram` collector to observe what actually leaves each process.

use std::collections::HashSet;
use std::os::unix::net::UnixDatagram;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::Duration;

use nix::sys::wait::{waitpid, WaitStatus};
use nix::unistd::{fork, ForkResult};
use once_cell::sync::Lazy;
use orb_dogd::{DogstatsdClient, MetricEmitter};

// Mirrors the real-world `static DATADOG: Lazy<DogstatsdClient> =
// Lazy::new(DogstatsdClient::new)`, but points at a per-test socket that the
// test process sets up before any fork.
static GOOD_SOCKET: OnceLock<String> = OnceLock::new();
static DATADOG: Lazy<DogstatsdClient> = Lazy::new(|| {
    DogstatsdClient::with_socket_path(
        GOOD_SOCKET
            .get()
            .expect("socket path set before first use")
            .clone(),
    )
});

static PREFORK_SOCKET: OnceLock<String> = OnceLock::new();
static DATADOG_PREFORK: Lazy<DogstatsdClient> = Lazy::new(|| {
    DogstatsdClient::with_socket_path(
        PREFORK_SOCKET
            .get()
            .expect("socket path set before first use")
            .clone(),
    )
});

/// Unique temp socket path for this test process; UDS paths must stay well
/// under the ~108 byte sun_path limit, so we keep it short.
fn socket_path(tag: &str) -> String {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("orb-dogd-{tag}-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&path);

    path.to_str().expect("temp path is utf8").to_owned()
}

/// Bind the collector the client worker will connect to. dogstatsd *connects*
/// to the path, so the listener must exist first.
fn bind_collector(path: &str) -> UnixDatagram {
    let sock = UnixDatagram::bind(path).expect("bind collector socket");
    sock.set_read_timeout(Some(Duration::from_secs(5)))
        .expect("set read timeout");

    sock
}

/// Read the value following `key:` in a statsd datagram, e.g. `child:2` -> 2,
/// `role:parent` -> "parent". dogstatsd appends tags as `|#k:v,...`.
fn tag_value(payload: &[u8], key: &str) -> Option<String> {
    let text = std::str::from_utf8(payload).ok()?;
    let start = text.find(key)? + key.len();
    let value: String = text[start..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();

    (!value.is_empty()).then_some(value)
}

/// Spawn allocator-churning background threads so the process is genuinely
/// multithreaded (and contending the allocator) at the instant of `fork`.
/// Returns the threads and a stop flag.
fn spawn_noise(count: usize) -> (Arc<AtomicBool>, Vec<thread::JoinHandle<()>>) {
    let stop = Arc::new(AtomicBool::new(false));
    let handles = (0..count)
        .map(|_| {
            let stop = Arc::clone(&stop);
            thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    let mut v: Vec<String> =
                        (0..64).map(|n| format!("noise-{n}")).collect();
                    v.sort();
                    std::hint::black_box(&v);
                    thread::sleep(Duration::from_millis(1));
                }
            })
        })
        .collect();

    (stop, handles)
}

/// The supported pattern: a multithreaded parent forks several children, each
/// of which first touches the global `Lazy<DogstatsdClient>` *after* the fork.
/// Every child must successfully deliver its metric to the collector.
#[test]
fn child_constructed_after_fork_delivers() {
    const CHILDREN: usize = 3;

    let path = socket_path("after");
    let collector = bind_collector(&path);
    GOOD_SOCKET.set(path).expect("set good socket path");

    // Parent is multithreaded across the fork. The global is deliberately NOT
    // touched here, so each child initializes its own worker post-fork.
    let (stop, noise) = spawn_noise(4);

    let mut children = Vec::new();
    for idx in 0..CHILDREN {
        // SAFETY: the child does a bounded amount of work then `_exit`s without
        // running atexit handlers, which is async-signal-safe practice.
        match unsafe { fork() }.expect("fork") {
            ForkResult::Child => {
                let ok = DATADOG
                    .incr("dogd.fork_test", [format!("child:{idx}")])
                    .is_ok();
                // Let the freshly spawned worker connect and flush the datagram.
                thread::sleep(Duration::from_millis(700));
                unsafe { libc::_exit(if ok { 0 } else { 1 }) };
            }
            ForkResult::Parent { child } => children.push(child),
        }
    }

    let mut delivered = HashSet::new();
    let mut buf = [0u8; 4096];
    for _ in 0..CHILDREN {
        match collector.recv(&mut buf) {
            Ok(n) => {
                let idx = tag_value(&buf[..n], "child:")
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or_else(|| panic!("unparsable datagram: {:?}", &buf[..n]));
                delivered.insert(idx);
            }
            Err(e) => panic!(
                "timed out after {}/{CHILDREN} children delivered: {e}",
                delivered.len()
            ),
        }
    }

    for child in children {
        match waitpid(child, None).expect("waitpid") {
            WaitStatus::Exited(_, 0) => {}
            other => panic!("child {child} exited abnormally: {other:?}"),
        }
    }

    stop.store(true, Ordering::Relaxed);
    noise
        .into_iter()
        .for_each(|h| h.join().expect("join noise"));

    let expected: HashSet<usize> = (0..CHILDREN).collect();
    assert_eq!(
        delivered, expected,
        "every forked child must deliver a metric"
    );
}

/// Fork-detection in action: a client built *before* the fork keeps working in
/// the parent, and after the fork its worker thread is gone in the child — so
/// the child's first emit detects the new PID and transparently respawns the
/// worker. Both processes must deliver.
#[test]
fn pre_fork_client_reconnects_in_child() {
    let path = socket_path("prefork");
    let collector = bind_collector(&path);
    PREFORK_SOCKET.set(path).expect("set socket path");

    // Build and connect the client BEFORE forking, and prove it works by
    // delivering a warmup. Receiving it means the worker drained the channel
    // and re-parked in `recv()`, so the child inherits a clean, empty channel.
    DATADOG_PREFORK
        .incr("dogd.warmup", ["role:warmup"])
        .expect("warmup enqueue");
    let mut buf = [0u8; 4096];
    let n = collector
        .recv(&mut buf)
        .expect("parent worker delivers warmup");
    assert_eq!(tag_value(&buf[..n], "role:").as_deref(), Some("warmup"));
    thread::sleep(Duration::from_millis(50)); // let the worker re-park in recv()

    let child = match unsafe { fork() }.expect("fork") {
        ForkResult::Child => {
            // The worker thread did not survive the fork; this emit must detect
            // the new PID, respawn the worker, and deliver.
            let ok = DATADOG_PREFORK
                .incr("dogd.fork_test", ["role:child"])
                .is_ok();
            // Let the respawned worker connect and flush the datagram.
            thread::sleep(Duration::from_millis(700));
            unsafe { libc::_exit(if ok { 0 } else { 1 }) };
        }
        ForkResult::Parent { child } => child,
    };

    // The parent's pre-fork client still works after the fork.
    DATADOG_PREFORK
        .incr("dogd.fork_test", ["role:parent"])
        .expect("parent enqueue");

    // Both the respawned child worker and the parent must deliver.
    let mut roles = HashSet::new();
    while !(roles.contains("child") && roles.contains("parent")) {
        match collector.recv(&mut buf) {
            Ok(n) => {
                if let Some(role) = tag_value(&buf[..n], "role:") {
                    roles.insert(role);
                }
            }
            Err(e) => panic!("timed out; delivered {roles:?}: {e}"),
        }
    }

    match waitpid(child, None).expect("waitpid") {
        WaitStatus::Exited(_, 0) => {}
        other => panic!("child failed to enqueue after reconnect: {other:?}"),
    }
}
