#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    orb_telemetry::TelemetryConfig::new()
        .with_journald(orb_attest::SYSLOG_IDENTIFIER)
        .init();
    orb_attest::main().await
}
