use clap::Parser;
use color_eyre::eyre::Result;
use orb_jobs_agent::args::Args;
use orb_jobs_agent::program::{self, Deps};
use orb_jobs_agent::settings::Settings;
use orb_jobs_agent::shell::Host;
use tracing::info;
use zenorb::Zenorb;

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

    let settings = Settings::from_args(args, "/mnt/scratch").await?;
    let connection = zbus::Connection::session().await?;

    info!("conecting to zenoh");
    let zenorb = Zenorb::from_cfg(zenorb::client_cfg(settings.zenoh_port))
        .orb_id(settings.orb_id.clone())
        .with_name("jobs-agent")
        .await?;

    let deps = Deps::new(Host, connection, zenorb, settings);

    program::run(deps).await?;

    info!("Shutting down jobs agent completed");
    Ok(())
}
