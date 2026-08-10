use clap::Parser;
use color_eyre::eyre::Result;
use orb_build_info::{make_build_info, BuildInfo};
use tracing::info;

use orb_health::run;

const BUILD_INFO: BuildInfo = make_build_info!();
const SYSLOG_IDENTIFIER: &str = "worldcoin-orbd";

#[derive(Parser, Debug)]
#[clap(version = BUILD_INFO.version, about)]
struct Args {}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    let tel_flusher = orb_telemetry::TelemetryConfig::new()
        .with_journald(SYSLOG_IDENTIFIER)
        .init();

    Args::parse();

    info!("Running file-sizes check");
    run().await;

    tel_flusher.flush_blocking();

    Ok(())
}
