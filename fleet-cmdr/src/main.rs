use clap::Parser;
use color_eyre::eyre::Result;
use orb_endpoints::{backend::Backend, endpoints::Endpoints};
use orb_fleet_cmdr::{
    args::Args,
    orb_info::{get_orb_id, get_orb_token},
    relay_connect, relay_disconnect,
    settings::Settings,
};
use orb_relay_messages::orb_commands::v1::OrbCommand;
use std::time::Duration;
use tracing::{debug, error, info};

const SYSLOG_IDENTIFIER: &str = "worldcoin-fleet-cmdr";

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    orb_telemetry::TelemetryConfig::new()
        .with_journald(SYSLOG_IDENTIFIER)
        .init();

    let args = Args::parse();
    let settings = Settings::get(&args)?;

    debug!(?settings, "starting fleet commander with settings");
    run(&settings).await
}

async fn run(settings: &Settings) -> Result<()> {
    info!("running fleet commander: {:?}", settings);

    let orb_id = get_orb_id().await?;
    let orb_token = get_orb_token().await?;
    let endpoints = Endpoints::new(Backend::from_env()?, &orb_id);

    let mut relay =
        relay_connect(&orb_id, orb_token, &endpoints, 3, Duration::from_secs(10))
            .await?;

    loop {
        match relay
            .wait_for_msg::<OrbCommand>(Duration::from_secs(10))
            .await
        {
            Ok(OrbCommand { commands: command }) => {
                println!("Received command: {:?}", command);
            }
            Err(e) => {
                error!("Error receiving message: {:?}", e);
                break;
            }
        }
    }

    relay_disconnect(&mut relay, Duration::from_secs(10), Duration::from_secs(10))
        .await?;

    Ok(())
}
