use std::env;
use tracing::info_span;

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let otel_config = orb_telemetry::OpenTelemetryConfig::new(
        "http://localhost:4317",
        orb_attest::SYSLOG_IDENTIFIER,
        "1.0.0",
        env::var("ORB_BACKEND").expect("ORB_BACKEND environment variable must be set").to_lowercase(),
    );

    let _telemetry_guard = orb_telemetry::TelemetryConfig::new()
        .with_journald(orb_attest::SYSLOG_IDENTIFIER)
        .with_opentelemetry(otel_config)
        .init();

    let main_span = info_span!("orb_attestation",
        version = "1.0.0",
        component = "main"
    );
    let _main_guard = main_span.enter();

    let app_span = info_span!("application_execution");
    let _app_guard = app_span.enter();

    orb_attest::main().await
}