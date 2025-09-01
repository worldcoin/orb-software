use color_eyre::eyre::Result;
use orb_info::orb_os_release::OrbOsRelease;

const SYSLOG_IDENTIFIER: &str = "worldcoin-connd";

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let tel_flusher = orb_telemetry::TelemetryConfig::new()
        .with_journald(SYSLOG_IDENTIFIER)
        .init();

    let result = orb_connd::run(OrbOsRelease::read().await?).await;

    tel_flusher.flush().await;

    result
}
