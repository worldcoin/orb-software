use std::time::Duration;

use color_eyre::eyre::Context as _;
use color_eyre::Result;
use opentelemetry::trace::{TraceContextExt as _, Tracer as _, TracerProvider as _};
use opentelemetry::KeyValue;
use tracing::{error, span};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt as _;

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    // Create a new OpenTelemetry trace pipeline that prints to stdout
    //let tracer = datadog_tracer()?;
    let tracer = otlp_tracer()?;

    //test_traces_on_tracer_directly(&tracer);

    // Create a tracing layer with the configured tracer
    let otel_layer = tracing_opentelemetry::layer()
        .with_tracer(tracer)
        .with_level(true);

    // Use the tracing subscriber `Registry`, or any other subscriber
    // that impls `LookupSpan`
    tracing_subscriber::registry()
        .with(otel_layer)
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Spans will be sent to the configured OpenTelemetry exporter
    {
        let root = span!(tracing::Level::ERROR, "app_start", work_units = 2);
        let _enter = root.enter();

        error!(foo = 2, "This event will be logged in the root span.");
        tokio::time::sleep(Duration::from_millis(4000)).await;
        error!("This event will be logged in the root span.");
    }
    tokio::time::sleep(Duration::from_millis(2000)).await;
    opentelemetry::global::shutdown_tracer_provider();
    tokio::time::sleep(Duration::from_millis(5000)).await;

    Ok(())
}

fn datadog_tracer() -> Result<opentelemetry_sdk::trace::Tracer> {
    opentelemetry_datadog::new_pipeline()
        .with_service_name("ryan-test")
        .with_http_client(reqwest::Client::new())
        .with_agent_endpoint("http://localhost:8126")
        .with_trace_config(
            opentelemetry_sdk::trace::Config::default()
                .with_sampler(opentelemetry_sdk::trace::Sampler::AlwaysOn)
                .with_id_generator(
                    opentelemetry_sdk::trace::RandomIdGenerator::default(),
                ),
        )
        .install_batch(opentelemetry_sdk::runtime::Tokio)
        .wrap_err("failed to install batch")
}

fn otlp_tracer() -> Result<opentelemetry_sdk::trace::Tracer> {
    let trace_provider = opentelemetry_sdk::trace::TracerProvider::builder()
        .with_resource(opentelemetry_sdk::Resource::new([
            KeyValue::new("service.name", "ryan-test"),
            KeyValue::new("service", "ryan-test"),
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
    Ok(trace_provider.tracer("ryan-test"))
}

fn test_traces_on_tracer_directly(tracer: &opentelemetry_sdk::trace::Tracer) {
    tracer.in_span("foo", |cx| {
        use opentelemetry::{Key, Value};
        let span = cx.span();
        span.set_attribute(KeyValue::new(
            Key::new("span.type"),
            Value::String("web".into()),
        ));
        span.set_attribute(KeyValue::new(
            Key::new("http.url"),
            Value::String("http://localhost:8080/foo".into()),
        ));
        span.set_attribute(KeyValue::new(
            Key::new("http.method"),
            Value::String("GET".into()),
        ));
        span.set_attribute(KeyValue::new(
            Key::new("http.status_code"),
            Value::I64(200),
        ));
    });
}
