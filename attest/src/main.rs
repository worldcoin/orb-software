#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let _telemetry_guard = orb_telemetry::TelemetryConfig::new(
        orb_attest::SYSLOG_IDENTIFIER,
        "1.0.0",
        "orb"
    )
        .with_journald(orb_attest::SYSLOG_IDENTIFIER)
        .with_opentelemetry(orb_telemetry::OpenTelemetryConfig::default())
        .init();

    orb_attest::main().await
}
