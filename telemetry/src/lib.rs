use std::io::IsTerminal as _;

use tracing::level_filters::LevelFilter;
use tracing_subscriber::{
    layer::SubscriberExt as _,
    EnvFilter,
};

use opentelemetry::{global, KeyValue};
use opentelemetry::trace::TracerProvider;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{
    trace::{self, Sampler},
    runtime::Tokio,
    Resource,
};
use tracing_subscriber::util::SubscriberInitExt;

/// A struct controlling how telemetry will be configured (logging + optional OpenTelemetry).
#[derive(Debug)]
pub struct TelemetryConfig {
    syslog_identifier: Option<String>,
    global_filter: EnvFilter,

    /// If true, enable OTLP tracing via OpenTelemetry.
    use_otel: bool,

    /// The service name used in opentelemetry's `Resource`.
    service_name: Option<String>,
    /// The service version used in opentelemetry's `Resource`.
    service_version: Option<String>,
    /// The environment used in opentelemetry's `Resource` (e.g. "prod", "stage").
    environment: Option<String>,

    /// If we create an OTEL `TracerProvider`, store it here for optional shutdown.
    tracer_provider: Option<trace::TracerProvider>,
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
                // Spans from dependencies are emitted only at the error level
                .parse_lossy(format!(
                    "info,zbus=error,h2=error,hyper=error,tonic=error,tower_http=error,{}",
                    std::env::var("RUST_LOG").unwrap_or_default()
                )),
            use_otel: false,
            service_name: None,
            service_version: None,
            environment: None,
            tracer_provider: None,
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
    /// Only do this if you actually need to deviate from orb's defaults.
    #[must_use]
    pub fn with_global_filter(self, filter: EnvFilter) -> Self {
        Self {
            global_filter: filter,
            ..self
        }
    }

    /// Enable OpenTelemetry/OTLP tracing.
    /// You can optionally provide a service name, version, and environment.
    /// If omitted, these will default to environment variables (`SERVICE_NAME`, `SERVICE_VERSION`, `ENVIRONMENT`) or hard-coded strings.
    #[must_use]
    pub fn with_opentelemetry(
        mut self,
        service_name: Option<String>,
        service_version: Option<String>,
        environment: Option<String>,
    ) -> Self {
        self.use_otel = true;
        self.service_name = service_name;
        self.service_version = service_version;
        self.environment = environment;
        self
    }

    /// Initialize the OpenTelemetry TracerProvider and set it globally, returning it for storing.
    fn init_opentelemetry(&mut self) -> Result<opentelemetry_sdk::trace::TracerProvider, Box<dyn std::error::Error>> {
        // Fallback to environment variables if the user did not supply them.
        let default_service_name =
            std::env::var("SERVICE_NAME").unwrap_or_else(|_| "orb-software".to_string());
        let default_service_version =
            std::env::var("SERVICE_VERSION").unwrap_or_else(|_| "0.1.0".to_string());
        let default_environment =
            std::env::var("ENVIRONMENT").unwrap_or_else(|_| "orb".to_string());

        let service_name = self
            .service_name
            .clone()
            .unwrap_or(default_service_name);
        let service_version = self
            .service_version
            .clone()
            .unwrap_or(default_service_version);
        let environment = self
            .environment
            .clone()
            .unwrap_or(default_environment);

        // Build an OpenTelemetry Resource with service metadata
        let resource = Resource::new(vec![
            KeyValue::new("service.name", service_name),
            KeyValue::new("service.version", service_version),
            KeyValue::new("deployment.environment", environment),
        ]);

        // OTLP endpoint from env or fallback
        let otlp_endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
            .unwrap_or_else(|_| "http://localhost:4317".to_string());

        let exporter = opentelemetry_otlp::new_exporter()
            .tonic()
            .with_endpoint(otlp_endpoint)
            .build_span_exporter()?;

        let trace_config = trace::config()
            .with_resource(resource)
            .with_sampler(Sampler::AlwaysOn);

        let tracer_provider = opentelemetry_sdk::trace::TracerProvider::builder()
            .with_config(trace_config)
            .with_batch_exporter(exporter, Tokio)
            .build();

        // Set the global tracer provider
        global::set_tracer_provider(tracer_provider.clone());

        // Use W3C propagation
        global::set_text_map_propagator(TraceContextPropagator::new());

        Ok(tracer_provider)
    }

    /// Try to initialize telemetry (journald/stderr + optional OTLP).
    /// Returns an error if something goes wrong setting up the subscriber stack.
    pub fn try_init(mut self) -> Result<(), tracing_subscriber::util::TryInitError> {
        // 1. If OTLP was requested, set up the tracer provider
        if self.use_otel {
            match self.init_opentelemetry() {
                Ok(provider) => {
                    self.tracer_provider = Some(provider);
                }
                Err(err) => {
                    eprintln!("Failed to initialize OTLP exporter: {err}");
                    // Degrade gracefully to journald/stderr logs
                }
            }
        }

        // 2. Base journald/stderr logging setup
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

        // 3. If OTLP tracing is available, attach a tracing-opentelemetry layer
        let otlp_layer = self.tracer_provider.as_ref().map(|provider| {
            let tracer = provider.tracer("orb-telemetry");
            tracing_opentelemetry::layer().with_tracer(tracer)
        });

        // 4. Build the final subscriber
        registry
            .with(tokio_console_layer)
            .with(stderr_layer)
            .with(journald_layer)
            .with(otlp_layer)
            .with(self.global_filter)
            .try_init()
    }

    /// Initializes telemetry, panicking if something goes wrong.
    pub fn init(self) {
        self.try_init().expect("failed to initialize orb-telemetry");
    }

    /// Optional shutdown hook to flush any pending OTLP spans.
    /// For journald/stderr, it's usually not necessary.
    pub fn shutdown_tracing(&self) {
        if self.tracer_provider.is_some() {
            // This ensures that spans are flushed before exit
            global::shutdown_tracer_provider();
        }
    }
}

pub async fn shutdown_tracing() {
    // Ensure all spans are flushed
    global::shutdown_tracer_provider();
}