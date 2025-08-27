use color_eyre::eyre::Result;

const SYSLOG_IDENTIFIER: &str = "worldcoin-connd";

// - [x] separate telemetry into telemetry
// - [ ] modem manager logic
//      - [ ] further breakdown
// - [ ] test with wifi qr code on dev orb, from boot
// - [ ] determine config

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
