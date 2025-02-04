use clap::Parser;
use color_eyre::eyre::Result;
use orb_endpoints::{backend::Backend, v1::Endpoints, OrbId};
use orb_fleet_cmdr::{args::Args, handlers::OrbCommandHandlers};
use orb_relay_client::{Auth, Client, ClientOpts};
use orb_relay_messages::relay::entity::EntityType;
use std::str::FromStr;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

const SYSLOG_IDENTIFIER: &str = "worldcoin-fleet-cmdr";

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    orb_telemetry::TelemetryConfig::new()
        .with_journald(SYSLOG_IDENTIFIER)
        .init();

    let args = Args::parse();
    run(&args).await
}

async fn run(args: &Args) -> Result<()> {
    info!("Starting fleet commander: {:?}", args);

    let orb_id = OrbId::from_str(args.orb_id.as_ref().unwrap())?;
    let endpoints = args.relay_host.clone().unwrap_or_else(|| {
        Endpoints::new(Backend::from_env().unwrap(), &orb_id)
            .relay
            .to_string()
    });
    let shutdown_token = CancellationToken::new();

    // Init Relay Client
    let opts = ClientOpts::entity(EntityType::Orb)
        .id(args.orb_id.clone().unwrap())
        .endpoint(endpoints.clone())
        .namespace(args.relay_namespace.clone().unwrap())
        .auth(Auth::Token(
            args.orb_token.clone().unwrap_or_default().into(),
        ))
        .build();
    let (relay_client, mut relay_handle) = Client::connect(opts);

    // Init Orb Command Handlers
    let handlers = OrbCommandHandlers::init();

    loop {
        tokio::select! {
            _ = shutdown_token.cancelled() => {
                info!("Shutting down fleet commander initiated");
                break;
            }
            _ = &mut relay_handle => {
                info!("Relay service shutdown detected");
                break;
            }
            msg = relay_client.recv() => {
                match msg {
                    Ok(command) => {
                        info!("received command: {:?}", command);
                        if let Err(e) = handlers.handle_orb_command(&command).await {
                            error!("error handling command: {:?}", e);
                        }
                    }
                    Err(e) => {
                        error!("error receiving command: {:?}", e);
                        break;
                    }
                }
            }
        }
    }

    info!("Shutting down fleet commander completed");
    Ok(())
}
