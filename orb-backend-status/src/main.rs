use clap::Parser;
use color_eyre::eyre::Result;
use orb_backend_status::args::Args;

const SYSLOG_IDENTIFIER: &str = "worldcoin-backend-status";

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let telemetry = orb_telemetry::TelemetryConfig::new()
        .with_journald(SYSLOG_IDENTIFIER)
        .init();

    let args = Args::parse();
    let result = orb_backend_status::run(&args).await;

    telemetry.flush().await;

    result
}


