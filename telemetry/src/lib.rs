use std::io::IsTerminal as _;

use tracing::level_filters::LevelFilter;
use tracing_subscriber::{
    layer::SubscriberExt as _, util::SubscriberInitExt as _, EnvFilter,
};

#[derive(Debug)]
pub struct TelemetryConfig {
    syslog_identifier: Option<String>,
    global_filter: EnvFilter,
}

impl TelemetryConfig {
    /// Provides all required arguments for telemetry configuration.
    /// - `log_identifier` will be used for journald, if appropriate.
    #[expect(clippy::new_without_default, reason = "may add required args later")]
    #[must_use]
    pub fn new() -> Self {
        Self {
            syslog_identifier: None,
            global_filter: EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy(),
        }
    }

    /// Enables journald, and uses the provided syslog identifier.
    ///
    /// If you run the application in a tty, stderr will be used instead.
    #[must_use]
    pub fn with_journald(self, syslog_identifier: &str) -> Self {
        Self {
            syslog_identifier: Some(syslog_identifier.to_owned()),
            ..self
        }
    }

    /// Override the global filter to a custom filter.
    /// Only do this if actually necessary to deviate from the orb's defaults.
    #[must_use]
    pub fn with_global_filter(self, filter: EnvFilter) -> Self {
        Self {
            global_filter: filter,
            ..self
        }
    }

    pub fn try_init(self) -> Result<(), tracing_subscriber::util::TryInitError> {
        let registry = tracing_subscriber::registry();
        // The type is only there to get it to compile.
        let tokio_console_layer: Option<tracing_subscriber::layer::Identity> = None;
        #[cfg(tokio_unstable)]
        let tokio_console_layer = console_subscriber::spawn();
        // Checking for a terminal helps detect if we are running under systemd.
        let journald_layer = if !std::io::stderr().is_terminal() {
            self.syslog_identifier.and_then(|syslog_identifier| {
                tracing_journald::layer()
                    .inspect_err(|err| {
                        eprintln!(
                            "failed connecting to journald socket. \
                        will write to stderr: {err}"
                        );
                    })
                    .map(|layer| layer.with_syslog_identifier(syslog_identifier))
                    .ok()
            })
        } else {
            None
        };
        let stderr_layer = journald_layer
            .is_none()
            .then(|| tracing_subscriber::fmt::layer().with_writer(std::io::stderr));
        assert!(stderr_layer.is_some() || journald_layer.is_some());
        registry
            .with(tokio_console_layer)
            .with(stderr_layer)
            .with(journald_layer)
            .with(self.global_filter)
            .try_init()
    }

    /// Initializes the telemetry config. Call this only once, at the beginning of the
    /// program.
    ///
    /// Calling this more than once or when another tracing subscriber is registered
    /// will cause a panic.
    pub fn init(self) {
        self.try_init().expect("failed to initialize orb-telemetry")
    }
}
