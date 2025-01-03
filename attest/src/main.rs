use tracing::info;

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    orb_telemetry::TelemetryConfig::new()
        .with_journald(orb_attest::SYSLOG_IDENTIFIER)
        .with_opentelemetry(
            Some(orb_attest::SYSLOG_IDENTIFIER.to_string()),
            Some("1.0.0".to_string()),
            Some("orb".to_string())
        )
        .init();

    orb_attest::main().await
}
