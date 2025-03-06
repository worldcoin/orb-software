use std::str::FromStr;

use clap::Parser;
use color_eyre::eyre::Result;
use orb_endpoints::{backend::Backend, v1::Endpoints};
use orb_fleet_cmdr::{args::Args, handlers::OrbCommandHandlers, job_client::JobClient};
use orb_info::{OrbId, TokenTaskHandle};
use orb_relay_client::{Auth, Client, ClientOpts};
use orb_relay_messages::{
    fleet_cmdr::v1::JobExecutionStatus, relay::entity::EntityType,
};
use tokio::sync::watch;
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
        let (_, receiver) = watch::channel(token);
        receiver
    } else {
        let connection = Connection::session().await?;
        _token_task = Some(TokenTaskHandle::spawn(&connection, &shutdown_token).await?);
        _token_task.as_ref().unwrap().token_recv.to_owned()
    };

    // Init Orb Command Handlers
    let handlers = OrbCommandHandlers::init().await;

    // Init Relay Client
    info!("Connecting to relay: {:?}", endpoints);
    let opts = ClientOpts::entity(EntityType::Orb)
        .id(args.orb_id.clone().unwrap())
        .endpoint(endpoints.clone())
        .namespace(args.relay_namespace.clone().unwrap())
        .auth(Auth::TokenReceiver(auth_token))
        .build();
    let (relay_client, mut relay_handle) = Client::connect(opts);
    let job_client = JobClient::new(
        relay_client.clone(),
        args.fleet_cmdr_id.clone().unwrap().as_str(),
        args.relay_namespace.clone().unwrap().as_str(),
    );

    // kick off init job poll
    let _ = job_client.request_next_job().await;

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
            msg = job_client.listen_for_job() => {
                if let Ok(job) = msg {
                    match handlers.handle_job_execution(&job, &job_client).await {
                        Ok(update) => {
                            if job_client.send_job_update(&update).await.is_ok()
                                && update.status == JobExecutionStatus::Succeeded as i32
                            {
                                let _ = job_client.request_next_job().await;
                            }
                        }
                        Err(e) => {
                            error!("error handling job execution: {:?}", e);
                        }
                    }
                }
            }
        }
    }

    info!("Shutting down fleet commander completed");
    Ok(())
}
