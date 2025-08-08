use clap::Parser;
use color_eyre::eyre::Result;
use orb_info::orb_os_release::{OrbOsPlatform, OrbOsRelease};
use orb_jobs_agent::args::Args;
use orb_jobs_agent::program::{self, Deps};
use orb_jobs_agent::settings::Settings;
use orb_jobs_agent::shell::Host;
use tracing::info;

const SYSLOG_IDENTIFIER: &str = "worldcoin-jobs-agent";

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let tel_flusher = orb_telemetry::TelemetryConfig::new()
        .with_journald(SYSLOG_IDENTIFIER)
        .init();

    let args = Args::parse();
    let result = run(&args).await;
    tel_flusher.flush().await;
    result
}

async fn run(args: &Args) -> Result<()> {
    info!("Starting jobs agent: {:?}", args);

    let orb_release = OrbOsRelease::read().await?;
    let store_path = match orb_release.orb_os_platform_type {
        OrbOsPlatform::Diamond => "/mnt/scratch",
        OrbOsPlatform::Pearl => "/mnt/update",
    };

    let deps = Deps::new(Host, Settings::from_args(args, store_path).await?);
    program::run(deps).await?;

    info!("Shutting down jobs agent completed");
    Ok(())
}
