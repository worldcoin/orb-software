//! Process-based agents.

use super::{Agent, Kill};
use crate::{
    port::{self, SharedPort, SharedSerializer},
    spawn_named_thread,
};
use close_fds::close_open_fds;
use futures::{future::Either, prelude::*};
use nix::{
    errno::Errno,
    sched::{unshare, CloneFlags},
    sys::signal::{self, Signal},
    unistd::Pid,
};
use rancor::Strategy;
use rkyv::{Archive, Deserialize, Serialize};
use std::{
    env,
    error::Error,
    fmt::Debug,
    io,
    os::{
        fd::{AsRawFd as _, FromRawFd as _, OwnedFd, RawFd},
        unix::process::{parent_id, ExitStatusExt as _},
    },
    pin::pin,
    process::{self, Stdio},
    sync::atomic::{AtomicBool, Ordering},
};
use thiserror::Error;
use tokio::{
    io::{AsyncBufReadExt as _, BufReader},
    process::{ChildStderr, ChildStdout, Command},
    runtime,
    sync::oneshot,
    task,
};

/// Environment variable to pass extra arguments to the agent process.
pub const ARGS_ENV: &str = "AGENTWIRE_PROCESS_ARGS";

const SHMEM_ENV: &str = "AGENTWIRE_PROCESS_SHMEM";
const PARENT_PID_ENV: &str = "AGENTWIRE_PROCESS_PARENT_PID";

static INIT_PROCESSES: AtomicBool = AtomicBool::new(false);

/// Error returned by [`Process::call`].
#[derive(Error, Debug)]
pub enum CallError<T: Debug> {
    /// Error returned by the agent.
    #[error("agent: {0}")]
    Agent(T),
    /// Error initializing the shared memory.
    #[error("shared memory: {0}")]
    SharedMemory(Errno),
}

/// Exit strategy returned from [`Process::exit_strategy`].
#[derive(Clone, Copy, Default, Debug)]
pub enum ExitStrategy {
    /// Close the port without restarting the agent.
    Close,
    /// Keep the port open and restart the agent.
    Restart,
    /// Keep the port open, restart the agent, and retry the latest input.
    #[default]
    Retry,
}

/// Additional settings for starting a new process.
pub trait Initializer: Send {
    /// File descriptors to keep open when starting a new process.
    #[must_use]
    fn keep_file_descriptors(&self) -> Vec<RawFd>;

    /// Additional environment variables for the process.
    #[must_use]
    fn envs(&self) -> Vec<(String, String)>;

    /// Optional path to a custom executable for this agent.
    ///
    /// When `Some(path)`, the specified executable is spawned instead of the
    /// current executable. This allows agents to run in separate binaries with
    /// different dependencies (e.g., a worker binary that links against a
    /// specific library that the main binary should not depend on).
    ///
    /// When `None` (default), the current executable is used.
    #[must_use]
    fn executable(&self) -> Option<std::path::PathBuf> {
        None
    }

    /// Optional seccomp policy for syscall filtering.
    ///
    /// When `Some`, the process will be sandboxed using minijail with the
    /// provided seccomp-BPF policy. When `None`, no seccomp filtering is applied.
    #[cfg(feature = "sandbox-minijail")]
    #[must_use]
    fn seccomp_policy(&self) -> Option<super::minijail::SeccompPolicy> {
        None
    }

    /// Optional filesystem isolation configuration.
    ///
    /// When `Some`, the process will have a restricted filesystem view using
    /// `pivot_root`. When `None`, the process has unrestricted filesystem access.
    #[cfg(feature = "sandbox-minijail")]
    #[must_use]
    fn pivot_root_fs_config(&self) -> Option<super::minijail::PivotRootFsConfig> {
        None
    }
}

/// Default initializer with no additional settings.
pub struct DefaultInitializer;

impl Initializer for DefaultInitializer {
    fn keep_file_descriptors(&self) -> Vec<RawFd> {
        Vec::new()
    }

    fn envs(&self) -> Vec<(String, String)> {
        Vec::new()
    }
}

/// Agent running on a dedicated OS process.
pub trait Process
where
    Self: Agent
        + SharedPort
        + Clone
        + Send
        + Debug
        + Archive
        + for<'a> Serialize<SharedSerializer<'a>>,
    <Self as Archive>::Archived:
        for<'a> Deserialize<Self, Strategy<(), rancor::Failure>>,
    Self::Input: Archive + for<'a> Serialize<SharedSerializer<'a>>,
    Self::Output: Archive + for<'a> Serialize<SharedSerializer<'a>>,
    <Self::Output as Archive>::Archived:
        for<'a> Deserialize<Self::Output, rancor::Strategy<(), rancor::Failure>>,
{
    /// Error type returned by the agent.
    type Error: Debug;

    /// Runs the agent event-loop inside a dedicated OS thread.
    fn run(self, port: port::RemoteInner<Self>) -> Result<(), Self::Error>;

    /// Spawns a new process running the agent event-loop and returns a handle
    /// for bi-directional communication with the agent.
    ///
    /// # Panics
    ///
    /// If [`init`] hasn't been called yet.
    fn spawn_process<Fut, F>(self, logger: F) -> (port::Outer<Self>, Kill)
    where
        F: Fn(&'static str, ChildStdout, ChildStderr) -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        assert!(
            INIT_PROCESSES.load(Ordering::Relaxed),
            "process-based agents are not initialized (missing call to \
             `agentwire::agent::process::init`)"
        );
        let (inner, outer) = port::new();
        let (send_kill_tx, send_kill_rx) = oneshot::channel();
        let (wait_kill_tx, wait_kill_rx) = oneshot::channel();
        let kill = async move {
            let _ = send_kill_tx.send(());
            wait_kill_rx.await.unwrap();
            tracing::info!("Process agent {} killed", Self::NAME);
        };
        let spawn_process =
            spawn_process_impl(self, inner, send_kill_rx, wait_kill_tx, logger);
        spawn_named_thread(format!("proc-ipc-{}", Self::NAME), || {
            let rt = runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(task::LocalSet::new().run_until(spawn_process));
        });
        (outer, kill.boxed())
    }

    /// Connects to the shared memory and calls the [`run`](Self::run) method.
    fn call(shmem: OwnedFd) -> Result<(), CallError<Self::Error>> {
        let mut inner = port::RemoteInner::<Self>::from_shared_memory(shmem)
            .map_err(CallError::SharedMemory)?;
        let agent = inner
            .init_state()
            .deserialize(Strategy::wrap(&mut ()))
            .unwrap();
        agent.run(inner).map_err(CallError::Agent)
    }

    /// When the agent process terminates, this method decides how to proceed.
    /// See [`ExitStrategy`] for available options.
    #[must_use]
    fn exit_strategy(_code: Option<i32>, _signal: Option<i32>) -> ExitStrategy {
        ExitStrategy::default()
    }

    /// Additional settings for starting a new process.
    #[must_use]
    fn initializer() -> impl Initializer {
        DefaultInitializer
    }
}

/// Initializes process-based agents.
///
/// This function must be called as early in the program lifetime as possible.
/// Everything before this function call gets duplicated for each process-based
/// agent.
pub fn init(
    call_process_agent: impl FnOnce(&str, OwnedFd) -> Result<(), Box<dyn Error>>,
) {
    match (env::var(SHMEM_ENV), env::var(PARENT_PID_ENV)) {
        (Ok(shmem), Ok(parent_pid)) => {
            let result = unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) };
            if result == -1 {
                eprintln!(
                    "Failed to set the parent death signal: {:#?}",
                    io::Error::last_os_error()
                );
                process::exit(1);
            }
            if parent_id() != parent_pid.parse::<u32>().unwrap() {
                // The parent exited before the above `prctl` call.
                process::exit(1);
            }
            let shmem_fd = unsafe {
                OwnedFd::from_raw_fd(
                    shmem
                        .parse::<RawFd>()
                        .expect("shared memory file descriptor to be an integer"),
                )
            };
            // Agent's name is the first argument.
            let argv0 = std::env::args().next().expect("argv[0] is not set");
            let name = argv0
                .strip_prefix("proc-")
                .expect("mega-agent process name should start with 'proc-'");
            match call_process_agent(name, shmem_fd) {
                Ok(()) => tracing::warn!("Agent {name} exited"),
                Err(err) => {
                    tracing::error!("Agent {name} exited with an error: {err:#?}");
                }
            }
            process::exit(1);
        }
        (Err(_), Err(_)) => {
            INIT_PROCESSES.store(true, Ordering::Relaxed);
        }
        (shmem, parent_pid) => {
            panic!(
                "Inconsistent state of the following environment variables: \
                 {SHMEM_ENV}={shmem:?}, {PARENT_PID_ENV}={parent_pid:?}, "
            );
        }
    }
}

/// Creates a default process agent logger.
pub async fn default_logger(
    agent_name: &'static str,
    stdout: ChildStdout,
    stderr: ChildStderr,
) {
    let mut stdout = BufReader::new(stdout).lines();
    let mut stderr = BufReader::new(stderr).lines();
    loop {
        match future::select(pin!(stdout.next_line()), pin!(stderr.next_line())).await {
            Either::Left((Ok(Some(line)), _)) => {
                tracing::info!("[{agent_name}] <STDOUT> {line}");
            }
            Either::Right((Ok(Some(line)), _)) => {
                tracing::info!("[{agent_name}] <STDERR> {line}");
            }
            Either::Left((Ok(None), _)) => {
                tracing::warn!("[{agent_name}] <STDOUT> closed");
                break;
            }
            Either::Right((Ok(None), _)) => {
                tracing::warn!("[{agent_name}] <STDERR> closed");
                break;
            }
            Either::Left((Err(err), _)) => {
                tracing::error!("[{agent_name}] <STDOUT> {err:#?}");
                break;
            }
            Either::Right((Err(err), _)) => {
                tracing::error!("[{agent_name}] <STDERR> {err:#?}");
                break;
            }
        }
    }
}

#[allow(clippy::too_many_lines)]
async fn spawn_process_impl<T: Process, Fut, F>(
    init_state: T,
    mut inner: port::Inner<T>,
    mut send_kill_rx: oneshot::Receiver<()>,
    wait_kill_tx: oneshot::Sender<()>,
    logger: F,
) where
    F: Fn(&'static str, ChildStdout, ChildStderr) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
    <T as Archive>::Archived: for<'a> Deserialize<T, Strategy<(), rancor::Failure>>,
    T::Input: Archive + for<'a> Serialize<SharedSerializer<'a>>,
    T::Output: Archive + for<'a> Serialize<SharedSerializer<'a>>,
    <T::Output as Archive>::Archived:
        for<'a> Deserialize<T::Output, rancor::Strategy<(), rancor::Failure>>,
{
    let mut recovered_inputs = Vec::new();
    loop {
        let (shmem_fd, close) = inner
            .into_shared_memory(T::NAME, &init_state, recovered_inputs)
            .expect("couldn't initialize shared memory");

        let initializer = T::initializer();

        // Use custom executable if provided, otherwise use current executable
        let exe = initializer.executable().unwrap_or_else(|| {
            env::current_exe().expect("couldn't determine current executable file")
        });

        let mut child_fds = initializer.keep_file_descriptors();
        child_fds.push(shmem_fd.as_raw_fd());

        #[cfg(feature = "sandbox-minijail")]
        let seccomp_policy = initializer.seccomp_policy();
        #[cfg(feature = "sandbox-minijail")]
        let pivot_root_fs_config = initializer.pivot_root_fs_config();

        let mut child = unsafe {
            Command::new(exe)
                .arg0(format!("proc-{}", T::NAME))
                .args(
                    env::var(ARGS_ENV)
                        .map(|args| {
                            shell_words::split(&args)
                                .expect("invalid process arguments")
                        })
                        .unwrap_or_default(),
                )
                .envs(initializer.envs())
                .env(SHMEM_ENV, shmem_fd.as_raw_fd().to_string())
                .env(PARENT_PID_ENV, process::id().to_string())
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .pre_exec(sandbox_agent)
                .pre_exec({
                    #[cfg(feature = "sandbox-minijail")]
                    let seccomp_policy = seccomp_policy.clone();
                    #[cfg(feature = "sandbox-minijail")]
                    let pivot_root_fs_config = pivot_root_fs_config.clone();

                    move || {
                        // Apply minijail sandboxing if configured
                        #[cfg(feature = "sandbox-minijail")]
                        if let Some(ref policy) = seccomp_policy {
                            super::minijail::apply_minijail(
                                policy,
                                pivot_root_fs_config.as_ref(),
                            )?;
                        }

                        // Close file descriptors (keep only what's needed)
                        close_open_fds(libc::STDERR_FILENO + 1, &child_fds);
                        Ok(())
                    }
                })
                .spawn()
                .expect("failed to spawn a sub-process")
        };
        drop(shmem_fd);
        drop(initializer);
        let pid = Pid::from_raw(child.id().unwrap().try_into().unwrap());
        task::spawn(logger(
            T::NAME,
            child.stdout.take().unwrap(),
            child.stderr.take().unwrap(),
        ));
        tracing::info!(
            "Process agent {} spawned with PID: {}",
            T::NAME,
            pid.as_raw()
        );
        match future::select(Box::pin(child.wait()), &mut send_kill_rx).await {
            Either::Left((status, _)) => {
                let status = status.expect("failed to run a sub-process");
                let (code, signal) = (status.code(), status.signal());
                if signal.is_some_and(|signal| signal == libc::SIGINT) {
                    tracing::warn!("Process agent {} exited on Ctrl-C", T::NAME);
                    break;
                }
                let exit_strategy = T::exit_strategy(code, signal);
                tracing::info!(
                    "Process agent {} exited with code {code:?} and signal {signal:?}, proceeding \
                     with {exit_strategy:?}",
                    T::NAME
                );
                (inner, recovered_inputs) =
                    close.await.expect("shared memory deinitialization failure");
                match exit_strategy {
                    ExitStrategy::Close => {
                        let _ = wait_kill_tx.send(());
                        break;
                    }
                    ExitStrategy::Restart => {
                        recovered_inputs.clear();
                    }
                    ExitStrategy::Retry => {}
                }
            }
            Either::Right((_kill, wait)) => {
                signal::kill(pid, Signal::SIGKILL)
                    .expect("failed to send SIGKILL to a sub-process");
                wait.await.expect("failed to kill a sub-process");
                close.await.expect("shared memory deinitialization failure");
                let _ = wait_kill_tx.send(());
                break;
            }
        }
    }
}

fn sandbox_agent() -> std::io::Result<()> {
    #[allow(unused_mut)]
    let mut flags = CloneFlags::CLONE_NEWUSER | CloneFlags::CLONE_NEWIPC;
    #[cfg(feature = "sandbox-network")]
    {
        flags |= CloneFlags::CLONE_NEWNET;
    }
    match unshare(flags) {
        Ok(()) => Ok(()),
        Err(err) => Err(err.into()),
    }
}
