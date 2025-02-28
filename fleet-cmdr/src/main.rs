use clap::Parser;
use color_eyre::eyre::Result;
use orb_endpoints::{backend::Backend, v1::Endpoints};
use orb_fleet_cmdr::{
    args::Args,
    handlers::{send_job_request, OrbCommandHandlers},
};
use orb_info::{OrbId, TokenTaskHandle};
use orb_relay_client::{Auth, Client, ClientOpts, QoS};
use orb_relay_messages::{
    fleet_cmdr::v1::{JobExecution, JobNotify},
    prost::{Message, Name},
    prost_types::Any,
    relay::entity::EntityType,
};
use std::str::FromStr;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};
use zbus::Connection;

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

    let orb_id = OrbId::from_str(args.orb_id.as_ref().unwrap())?;
    let endpoints = args.relay_host.clone().unwrap_or_else(|| {
        Endpoints::new(Backend::from_env().expect("Backend env error"), &orb_id)
            .relay
            .to_string()
    });
    let shutdown_token = CancellationToken::new();

    // Get token from DBus
    let mut _token_task: Option<TokenTaskHandle> = None;
    let auth_token = if let Some(token) = args.orb_token.clone() {
        token
    } else {
        let connection = Connection::session().await?;
        _token_task = Some(TokenTaskHandle::spawn(&connection, &shutdown_token).await?);
        _token_task.as_ref().unwrap().token_recv.borrow().to_owned()
    };

    // Init Orb Command Handlers
    let handlers = OrbCommandHandlers::init().await;

    // Init Relay Client
    info!("Connecting to relay: {:?}", endpoints);
    let opts = ClientOpts::entity(EntityType::Orb)
        .id(args.orb_id.clone().unwrap())
        .endpoint(endpoints.clone())
        .namespace(args.relay_namespace.clone().unwrap())
        .auth(Auth::Token(auth_token.into()))
        .build();
    let (relay_client, mut relay_handle) = Client::connect(opts);

    // kick off init job poll
    let _ = send_job_request(
        &relay_client,
        args.fleet_cmdr_id.as_ref().unwrap(),
        args.relay_namespace.as_ref().unwrap(),
    )
    .await;

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
                        let any = Any::decode(command.payload.as_slice()).unwrap();
                        if any.type_url == JobNotify::type_url() {
                            let job = JobNotify::decode(any.value.as_slice()).unwrap();
                            info!("received job notify: {:?}", job);
                            let _ = send_job_request(&relay_client,
                                args.fleet_cmdr_id.as_ref().unwrap(),
                                args.relay_namespace.as_ref().unwrap(),
                            )
                            .await;
                        } else if any.type_url == JobExecution::type_url() {
                            let job = JobExecution::decode(any.value.as_slice()).unwrap();
                            info!("received job execution: {:?}", job);
                            match handlers.handle_job_execution(&job, &relay_client).await {
                                Ok(update) => {
                                    info!("sending job update: {:?}", update);
                                    let any = Any::from_msg(&update).unwrap();
                                    command
                                        .reply(any.encode_to_vec(), QoS::AtLeastOnce)
                                        .await
                                        .unwrap();
                                }
                                Err(e) => {
                                    error!("error handling job execution: {:?}", e);
                                }
                            }
                        } else {
                            error!("unknown job message type: {:?}", any.type_url);
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
