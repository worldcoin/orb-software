use std::io::IsTerminal as _;

use thiserror::Error;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::{
    layer::SubscriberExt as _,
    EnvFilter,
    util::SubscriberInitExt,
};

use opentelemetry::{global, KeyValue};
use opentelemetry::trace::TracerProvider;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{
    trace::{self, Sampler},
    runtime::Tokio,
    Resource,
};
use opentelemetry_sdk::propagation::TraceContextPropagator;

#[derive(Error, Debug)]
pub enum OrbTelemetryError {
    #[error("Failed to initialize OpenTelemetry: {0}")]
    OpenTelemetryInit(#[from] opentelemetry::trace::TraceError),
    #[error("Failed to initialize subscriber: {0}")]
    SubscriberInit(#[from] tracing_subscriber::util::TryInitError),
}

/// Configuration for OpenTelemetry tracing.
#[derive(Debug, Clone)]
pub struct OpenTelemetryConfig {
    /// The endpoint to send OTLP data to
    pub endpoint: String,
    /// The name of the service
    pub service_name: String,
    /// The version of the service
    pub service_version: String,
    /// The deployment environment
    pub environment: String,
}

impl Default for OpenTelemetryConfig {
    fn default() -> Self {
        Self {
            endpoint: "http://localhost:4317".to_string(),
            service_name: String::new(),
            service_version: String::new(),
            environment: String::new(),
        }
    }
}

impl OpenTelemetryConfig {
    /// Creates a new OpenTelemetry configuration.
    pub fn new(
        endpoint: impl Into<String>,
        service_name: impl Into<String>,
        service_version: impl Into<String>,
        environment: impl Into<String>,
    ) -> Self {
        Self {
            endpoint: endpoint.into(),
            service_name: service_name.into(),
            service_version: service_version.into(),
            environment: environment.into(),
        }
    }

    /// Initialize the OpenTelemetry TracerProvider and returns a tracer.
    fn init_tracer(&self) -> Result<(trace::TracerProvider, trace::Tracer), OrbTelemetryError> {
        // Build an OpenTelemetry Resource with service metadata
        let resource = Resource::new(vec![
            KeyValue::new("service.name", self.service_name.clone()),
            KeyValue::new("service.version", self.service_version.clone()),
            KeyValue::new("deployment.environment", self.environment.clone()),
        ]);

        let exporter = opentelemetry_otlp::new_exporter()
            .tonic()
            .with_endpoint(&self.endpoint)
            .build_span_exporter()?;

        let trace_config = trace::config()
            .with_resource(resource)
            .with_sampler(Sampler::AlwaysOn);

        let tracer_provider = trace::TracerProvider::builder()
            .with_config(trace_config)
            .with_batch_exporter(exporter, Tokio)
            .build();

        let tracer = tracer_provider.tracer("telemetry");

        global::set_tracer_provider(tracer_provider.clone());
        global::set_text_map_propagator(TraceContextPropagator::new());

        Ok((tracer_provider, tracer))
    }
}

/// A struct controlling how telemetry will be configured (logging + optional OpenTelemetry).
#[derive(Debug)]
pub struct TelemetryConfig {
    syslog_identifier: Option<String>,
    global_filter: EnvFilter,
    otel: Option<OpenTelemetryConfig>,
}

/// Handles cleanup of telemetry resources on drop.
#[must_use]
pub struct TelemetryShutdownHandler;

impl Drop for TelemetryShutdownHandler {
    fn drop(&mut self) {
        global::shutdown_tracer_provider();
    }
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl TelemetryConfig {
    /// Creates a new telemetry configuration.
    pub fn new() -> Self {
        Self {
            syslog_identifier: None,
            global_filter: EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                // Spans from dependencies are emitted only at the error level
                .parse_lossy("info,zbus=error,h2=error,hyper=error,tonic=error,tower_http=error"),
            otel: None,
        }
    }

    /// Enables journald, and uses the provided syslog identifier.
    ///
    /// If you run the application in a tty, stderr will be used instead.
    #[must_use]
    pub fn with_journald(self, syslog_identifier: impl Into<String>) -> Self {
        Self {
            syslog_identifier: Some(syslog_identifier.into()),
            ..self
        }
    }

    /// Override the global filter to a custom filter.
    /// Only do this if you actually need to deviate from the defaults.
    #[must_use]
    pub fn with_global_filter(self, filter: EnvFilter) -> Self {
        Self {
            global_filter: filter,
            ..self
        }
    }

    /// Enable OpenTelemetry/OTLP tracing with the specified configuration.
    #[must_use]
    pub fn with_opentelemetry(self, config: OpenTelemetryConfig) -> Self {
        Self {
            otel: Some(config),
            ..self
        }
    }

    /// Try to initialize telemetry (journald/stderr + optional OTLP).
    pub fn try_init(self) -> Result<TelemetryShutdownHandler, OrbTelemetryError> {
        // Set up the tracer provider if OTLP was requested
        let tracer = if let Some(otel_config) = self.otel.as_ref() {
            match otel_config.init_tracer() {
                Ok((_provider, tracer)) => Some(tracer),
                Err(err) => {
                    eprintln!("Failed to initialize OTLP exporter: {err}");
                    None
                }
            }
        } else {
            None
        };

        // Base journald/stderr logging setup
        let registry = tracing_subscriber::registry();

        // If tokio_unstable is enabled, we can gather runtime metrics
        let tokio_console_layer: Option<tracing_subscriber::layer::Identity> = None;
        #[cfg(tokio_unstable)]
        let tokio_console_layer = console_subscriber::spawn();

        // If we're not attached to a terminal, assume journald is the intended output
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

        // If journald is not available or we're in a TTY, fallback to stderr
        let stderr_layer = journald_layer
            .is_none()
            .then(|| tracing_subscriber::fmt::layer().with_writer(std::io::stderr));

        // If OTLP tracing is available, attach a tracing-opentelemetry layer
        let otlp_layer = tracer.map(|tracer| {
            tracing_opentelemetry::layer().with_tracer(tracer)
        });

        // Build the final subscriber
        registry
            .with(tokio_console_layer)
            .with(stderr_layer)
            .with(journald_layer)
            .with(otlp_layer)
            .with(self.global_filter)
            .try_init()?;

        Ok(TelemetryShutdownHandler)
    }

    /// Initializes telemetry, panicking if something goes wrong.
    /// Returns a shutdown handler that will clean up resources when dropped.
    pub fn init(self) -> TelemetryShutdownHandler {
        self.try_init().expect("failed to initialize telemetry")
    }
}
