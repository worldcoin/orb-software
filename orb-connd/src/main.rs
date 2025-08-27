use color_eyre::eyre::Result;

const SYSLOG_IDENTIFIER: &str = "worldcoin-connd";

// - [ ] separate telemetry into telemetry
// - [ ] determine config
// - [ ] modem manager logic
//      - [ ] further breakdown
// - [ ] test with wifi qr code on dev orb, from boot

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let tel_flusher = orb_telemetry::TelemetryConfig::new()
        .with_journald(SYSLOG_IDENTIFIER)
        .init();

    let result = orb_connd::run().await;

    tel_flusher.flush().await;

    result
}
