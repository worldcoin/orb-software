use clap::Parser;
use color_eyre::eyre::Result;
use orb_endpoints::{backend::Backend, v1::Endpoints};
use orb_fleet_cmdr::{args::Args, handlers::JobActionHandlers};
use orb_info::{OrbId, TokenTaskHandle};
use orb_relay_client::{Auth, Client, ClientOpts, SendMessage};
use orb_relay_messages::{
    fleet_cmdr::v1::JobRequestNext, prost::Message, prost_types::Any,
    relay::entity::EntityType,
};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

const SYSLOG_IDENTIFIER: &str = "worldcoin-fleet-cmdr";

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
    info!("Starting fleet commander: {:?}", args);

    let orb_id = OrbId::read().await?;
    let shutdown_token = CancellationToken::new();
    let connection = zbus::ConnectionBuilder::session()?.build().await?;
    let orb_token = TokenTaskHandle::spawn(&connection, &shutdown_token).await?;
    let endpoints = args.relay_host.clone().unwrap_or_else(|| {
        Endpoints::new(Backend::from_env().expect("Backend env error"), &orb_id)
            .relay
            .to_string()
    });

    // Init Orb Command Handlers
    let handlers = JobActionHandlers::init().await;

    // Init Relay Client
    info!("Connecting to relay: {:?}", endpoints);
    let opts = ClientOpts::entity(EntityType::Orb)
        .id(args.orb_id.clone().unwrap())
        .endpoint(endpoints.clone())
        .namespace(args.relay_namespace.clone().unwrap())
        .auth(Auth::Token(orb_token.value().into()))
        .build();
    let (relay_client, mut relay_handle) = Client::connect(opts);

    // kick off init job poll
    let msg = Any::from_msg(&JobRequestNext::default()).unwrap();
    match relay_client
        .send(
            SendMessage::to(EntityType::Service)
                .id(args.fleet_cmdr_id.clone().unwrap())
                .namespace(args.relay_namespace.clone().unwrap())
                .payload(msg.encode_to_vec()),
        )
        .await
    {
        Ok(_) => {
            info!("sent initial job request");
        }
        Err(e) => {
            error!("error sending initial job request: {:?}", e);
        }
    }

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
                        if let Err(e) = handlers.handle_msg(&command).await {
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
