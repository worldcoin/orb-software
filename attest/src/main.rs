#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let telemetry = orb_telemetry::TelemetryConfig::new()
        .with_journald(orb_attest::SYSLOG_IDENTIFIER)
        .init();
    let result = orb_attest::main().await;
    telemetry.flush().await;
    result
}
