//! Standardized telemetry for the orb.
//!
//! See the `examples` dir. Start with [`TelemetryConfig::new()`].

use std::io::{IsTerminal as _, Write as _};

pub use std::collections::HashMap;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::{
    layer::SubscriberExt as _, util::SubscriberInitExt as _, EnvFilter,
};

#[cfg(feature = "otel")]
mod _otel_stuff {
    pub use opentelemetry::propagation::TextMapPropagator;
    pub use opentelemetry::trace::TracerProvider;
    pub use opentelemetry::KeyValue;
    pub use opentelemetry_sdk::propagation::TraceContextPropagator;
    pub use tokio::io::AsyncWriteExt as _;
    pub use tracing_opentelemetry::OpenTelemetrySpanExt;
    pub use tracing_subscriber::layer::Layer;
}
#[cfg(feature = "otel")]
use self::_otel_stuff::*;

/// Represents the attributes that will be attached to opentelemetry data for this
/// service.
///
/// This is similar in concept to datadog Tags.
#[derive(Debug)]
#[cfg(feature = "otel")]
pub struct OpentelemetryAttributes {
    /// Name of the service
    pub service_name: String,
    /// Version of the service. We suggest getting this from `orb-build-info`.
    pub service_version: String,
    /// Any additional attributes
    pub additional_otel_attributes: Vec<opentelemetry::KeyValue>,
}

#[derive(Debug)]
#[cfg(feature = "otel")]
pub struct OpentelemetryConfig {
    /// The tracer that will be used for opentelmetry.
    pub tracer_provider: opentelemetry_sdk::trace::TracerProvider,
    /// The name used for the tracer. Should be set to the service name.
    pub tracer_name: String,
    /// The layer filter that will be used, might be None.
    pub filter: Option<tracing_subscriber::filter::Targets>,
}

#[cfg(feature = "otel")]
impl OpentelemetryConfig {
    pub fn new(
        attrs: OpentelemetryAttributes,
    ) -> Result<Self, opentelemetry::trace::TraceError> {
        // The otlp exporter for opentelemetry *requires* that a tokio runtime is
        // present. To make this easier and less error prone in synchronous use cases,
        // we create a new single threaded runtime if no runtime is currently present.
        let rt = tokio::runtime::Handle::try_current().unwrap_or_else(|_| {
            let (tx, rx) = tokio::sync::oneshot::channel();
            std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();
                tx.send(rt.handle().clone()).ok();
                rt.block_on(std::future::pending::<()>())
            });
            rx.blocking_recv()
                .expect("failed to create new tokio runtime")
        });
        let _tokio_ctx = rt.enter();

        let tracer_provider = opentelemetry_sdk::trace::TracerProvider::builder()
            .with_resource(opentelemetry_sdk::Resource::new([
                KeyValue::new("service.name", attrs.service_name.clone()),
                KeyValue::new("service", attrs.service_name.clone()),
            ]))
            .with_sampler(opentelemetry_sdk::trace::Sampler::AlwaysOn)
            .with_id_generator(opentelemetry_sdk::trace::RandomIdGenerator::default())
            .with_batch_exporter(
                opentelemetry_otlp::SpanExporter::builder()
                    .with_tonic()
                    .build()?,
                opentelemetry_sdk::runtime::Tokio,
            )
            .build();

        Ok(Self {
            tracer_provider,
            tracer_name: attrs.service_name,
            filter: None,
        })
    }

    pub fn with_filter(self, filter: tracing_subscriber::filter::Targets) -> Self {
        Self {
            filter: Some(filter),
            ..self
        }
    }
}

/// The toplevel config for the orb-telemetry crate. Start here.
#[derive(Debug)]
pub struct TelemetryConfig {
    syslog_identifier: Option<String>,
    global_filter: EnvFilter,
    #[cfg(feature = "otel")]
    otel_cfg: Option<OpentelemetryConfig>,
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
            #[cfg(feature = "otel")]
            otel_cfg: None,
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

    #[cfg(feature = "otel")]
    #[must_use]
    pub fn with_opentelemetry(self, cfg: OpentelemetryConfig) -> Self {
        Self {
            otel_cfg: Some(cfg),
            ..self
        }
    }

    pub fn try_init(
        self,
    ) -> Result<TelemetryFlusher, tracing_subscriber::util::TryInitError> {
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

        #[cfg(feature = "otel")]
        let (otel_layer, otel_provider) = if let Some(otel_cfg) = self.otel_cfg {
            let tracer = otel_cfg.tracer_provider.tracer(otel_cfg.tracer_name);
            let layer = tracing_opentelemetry::OpenTelemetryLayer::new(tracer)
                .with_filter(otel_cfg.filter);
            (Some(layer), Some(otel_cfg.tracer_provider))
        } else {
            (None, None)
        };

        let registry = registry
            .with(tokio_console_layer)
            .with(stderr_layer)
            .with(journald_layer);
        #[cfg(feature = "otel")]
        let registry = registry.with(otel_layer);
        registry.with(self.global_filter).try_init()?;

        Ok(TelemetryFlusher {
            #[cfg(feature = "otel")]
            otel_provider,
        })
    }

    /// Initializes the telemetry config. Call this only once, at the beginning of the
    /// program.
    ///
    /// Calling this more than once or when another tracing subscriber is registered
    /// will cause a panic.
    pub fn init(self) -> TelemetryFlusher {
        self.try_init().expect("failed to initialize orb-telemetry")
    }
}

/// Allows flushing all telemetry logs.
#[must_use = "call .join at the end of the program to flush logs, otherwise they may get lost"]
pub struct TelemetryFlusher {
    #[cfg(feature = "otel")]
    otel_provider: Option<opentelemetry_sdk::trace::TracerProvider>,
}

impl TelemetryFlusher {
    /// Call this at the end of the program.
    pub async fn flush(self) {
        #[cfg(feature = "otel")]
        {
            let task = tokio::task::spawn_blocking(|| self.blocking_work());
            task.await.unwrap();
            tokio::io::stderr().flush().await.ok();
            tokio::io::stdout().flush().await.ok();
        }
        #[cfg(not(feature = "otel"))]
        {
            // technically blocks, but no one really cares for stderr/out.
            std::io::stderr().flush().ok();
            std::io::stdout().flush().ok();
        }
    }

    /// Call this at the end of the program.
    pub fn flush_blocking(self) {
        self.blocking_work();
        std::io::stderr().flush().ok();
        std::io::stdout().flush().ok();
    }

    #[cfg_attr(not(feature = "otel"), expect(unused_mut))]
    fn blocking_work(mut self) {
        #[cfg(feature = "otel")]
        if let Some(otel_provider) = self.otel_provider.take() {
            for flush_result in otel_provider.force_flush() {
                if let Err(err) = flush_result {
                    // use stderr because tracing is not working
                    eprintln!("failed to flush opentelemetry tracers, writing to stderr: {err:?}");
                }
            }
            if let Err(err) = otel_provider.shutdown() {
                // use stderr because tracing is not working
                eprintln!("failed to shutdown opentelemetry tracers, writing to stderr: {err:?}");
            }
        }
    }
}

/// Shared struct to add to any struct that needs to be traced over dbus.
#[derive(Debug, Default, Clone, Eq, PartialEq)]
#[cfg_attr(
    feature = "zbus-tracing",
    derive(
        zbus::zvariant::Type,
        zbus::zvariant::SerializeDict,
        zbus::zvariant::DeserializeDict,
    ),
    zvariant(signature = "a{sv}")
)]
pub struct TraceCtx {
    pub ctx: HashMap<String, String>,
}

/// # TraceCtx: Trace Context
///
/// TraceCtx is used to extract and inject the current trace context.
/// This is useful for tracing across different processes, such as when using
/// the `zbus` crate to call remote methods.
///
/// ```
/// use orb_telemetry::TraceCtx;
///
/// fn remote_method(param: &str, trace_ctx: TraceCtx) {
///     let span = tracing::span!(tracing::Level::INFO, "remote_method");       
///     trace_ctx.apply(&span);
///     // ...
/// }
///
/// let result = remote_method("param", TraceCtx::collect());
/// ```
///
impl TraceCtx {
    pub fn collect() -> Self {
        #[cfg(feature = "otel")]
        {
            let mut carrier = HashMap::new();
            TraceContextPropagator::new()
                .inject_context(&tracing::Span::current().context(), &mut carrier);
            Self { ctx: carrier }
        }
        #[cfg(not(feature = "otel"))]
        {
            Self {
                ctx: HashMap::new(),
            }
        }
    }

    #[cfg(feature = "otel")]
    pub fn apply(&self, span: &tracing::Span) {
        if !self.ctx.is_empty() {
            let parent_context = TraceContextPropagator::new().extract(&self.ctx);
            span.set_parent(parent_context);
        }
    }

    #[cfg(not(feature = "otel"))]
    pub fn inject(&self, _span: &tracing::Span) {}
}
