use clap::Parser;
use color_eyre::eyre::{eyre, Result};
use orb_endpoints::{backend::Backend, endpoints::Endpoints, OrbId};
use orb_fleet_cmdr::{
    args::Args,
    orb_info::{get_orb_id, get_orb_token},
    settings::Settings,
};
use orb_relay_client::client::Client;
use orb_relay_messages::common;
use std::{borrow::Cow, path::Path, time::Duration};
use tracing::{debug, error, info};

const CFG_DEFAULT_PATH: &str = "/etc/orb_fleet_cmdr.conf";
const ENV_VAR_PREFIX: &str = "ORB_FLEET_CMDR_";
const CFG_ENV_VAR: &str = const_format::concatcp!(ENV_VAR_PREFIX, "CONFIG");
const SYSLOG_IDENTIFIER: &str = "worldcoin-fleet-cmdr";
const ORB_FLEET_CMDR_NAMESPACE: &str = "orb-fleet-cmdr";
const ORB_RELAY_DEST_ID: &str = "orb-fleet-cmdr";

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    orb_telemetry::TelemetryConfig::new()
        .with_journald(SYSLOG_IDENTIFIER)
        .init();

    let args = Args::parse();
    let config_path = get_config_source(&args);

    let settings = Settings::get(&args, config_path, ENV_VAR_PREFIX)?;

    debug!(?settings, "starting fleet commander with settings");
    run(&settings).await
}

fn get_config_source(args: &Args) -> Cow<'_, Path> {
    if let Some(config) = &args.config {
        info!("using config provided by command line argument: `{config}`");
        Cow::Borrowed(config.as_ref())
    } else if let Some(config) = figment::providers::Env::var(CFG_ENV_VAR) {
        info!("using config set in environment variable `{CFG_ENV_VAR}={config}`");
        Cow::Owned(std::path::PathBuf::from(config))
    } else {
        info!("using default config at `{CFG_DEFAULT_PATH}`");
        Cow::Borrowed(CFG_DEFAULT_PATH.as_ref())
    }
}

async fn run(settings: &Settings) -> Result<()> {
    info!("running fleet commander: {:?}", settings);

    let orb_id = get_orb_id().await?;
    let orb_token = get_orb_token().await?;
    let endpoints = Endpoints::new(Backend::from_env()?, &orb_id);

    let mut relay =
        relay_connect(&orb_id, orb_token, &endpoints, 3, Duration::from_secs(10))
            .await?;

    // TODO: Implement the main loop

    relay_disconnect(&mut relay, Duration::from_secs(10), Duration::from_secs(10))
        .await?;

    Ok(())
}

async fn relay_connect(
    orb_id: &OrbId,
    orb_token: String,
    endpoints: &Endpoints,
    reties: u32,
    timeout: Duration,
) -> Result<Client> {
    let mut relay = Client::new_as_orb(
        endpoints.relay.to_string(),
        orb_token,
        orb_id.to_string(),
        ORB_RELAY_DEST_ID.to_string(),
        ORB_FLEET_CMDR_NAMESPACE.to_string(),
    );
    if let Err(e) = relay.connect().await {
        return Err(eyre!("Relay: Failed to connect: {e}"));
    }
    for _ in 0..reties {
        if let Ok(()) = relay
            .send_blocking(
                common::v1::AnnounceOrbId {
                    orb_id: orb_id.to_string(),
                    mode_type: common::v1::announce_orb_id::ModeType::SelfServe.into(),
                    hardware_type: common::v1::announce_orb_id::HardwareType::Pearl
                        .into(),
                },
                timeout,
            )
            .await
        {
            // Happy path. We have successfully announced and acknowledged the OrbId.
            return Ok(relay);
        }
        error!("Relay: Failed to AnnounceOrbId. Retrying...");
        relay.reconnect().await?;
        if relay.has_pending_messages().await? > 0 {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
    Err(eyre!(
        "Relay: Failed to send AnnounceOrbId after a reconnect"
    ))
}

async fn relay_disconnect(
    relay: &mut Client,
    wait_for_pending_messages: Duration,
    wait_for_shutdown: Duration,
) -> Result<()> {
    relay
        .graceful_shutdown(wait_for_pending_messages, wait_for_shutdown)
        .await;
    Ok(())
}
