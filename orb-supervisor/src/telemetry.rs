use tap::prelude::*;
use tracing::metadata::LevelFilter;
use tracing_subscriber::{
    filter,
    fmt::MakeWriter,
    prelude::*,
    util::TryInitError,
};

const SYSLOG_IDENTIFIER: &str = "worldcoin-supervisor";

fn is_tty_interactive() -> bool {
    unsafe { libc::isatty(libc::STDIN_FILENO) == 1 }
}

pub struct ExecContext;
pub struct TestContext;

pub trait Context: private::Sealed {
    const ENABLE_TELEMETRY: bool = false;
}

impl Context for ExecContext {
    const ENABLE_TELEMETRY: bool = true;
}
impl Context for TestContext {}

mod private {
    use super::{
        ExecContext,
        TestContext,
    };
    pub trait Sealed {}

    impl Sealed for ExecContext {}
    impl Sealed for TestContext {}
}

pub fn start<C: Context, W>(env_filter: LevelFilter, sink: W) -> Result<(), TryInitError>
where
    W: for<'a> MakeWriter<'a> + Send + Sync + 'static,
{
    let env_filter = filter::EnvFilter::builder()
        .with_default_directive(env_filter.into())
        .from_env_lossy();

    let mut fmt = None;
    let mut journald = None;

    if C::ENABLE_TELEMETRY && !is_tty_interactive() {
        journald = tracing_journald::layer()
            .tap_err(|err| {
                eprintln!("failed connecting to journald socket; will write to stdout: {err}");
            })
            .map(|layer| layer.with_syslog_identifier(SYSLOG_IDENTIFIER.into()))
            .ok();
    }
    if journald.is_none() {
        fmt = Some(tracing_subscriber::fmt::layer().with_writer(sink));
    }

    tracing_subscriber::registry()
        .with(fmt)
        .with(journald)
        .with(env_filter)
        .try_init()
}
