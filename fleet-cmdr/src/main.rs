use std::str::FromStr;

use clap::Parser;
use color_eyre::eyre::Result;
use orb_endpoints::{backend::Backend, v1::Endpoints};
use orb_fleet_cmdr::{
    args::Args,
    handlers::OrbCommandHandlers,
    job_client::JobClient,
    orchestrator::{JobConfig, JobRegistry},
};
use orb_info::{OrbId, TokenTaskHandle};
use orb_relay_client::{Auth, Client, ClientOpts};
use orb_relay_messages::relay::entity::EntityType;
use tokio::sync::{oneshot, watch};
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

    // Initialize orchestrator components
    let job_registry = JobRegistry::new();
    let _job_config = JobConfig::new();
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

    // Create a oneshot to trigger initial job request (use Option to handle consumption)
    let (initial_trigger_tx, initial_trigger_rx) = oneshot::channel::<()>();
    let mut initial_trigger_rx = Some(initial_trigger_rx);
    // Send immediately to trigger the initial request
    initial_trigger_tx.send(()).ok();

    // Main orchestrator loop
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
            _ = async {
                if let Some(rx) = initial_trigger_rx.take() {
                    let _ = rx.await.ok();
                } else {
                    // If already consumed, return a future that never completes
                    std::future::pending::<()>().await
                }
            } => {
                // Make initial job request now that we're listening
                if let Err(e) = job_client.request_next_job().await {
                    error!("Failed to request initial job: {:?}", e);
                }
            }
            msg = job_client.listen_for_job() => {
                if let Ok(job) = msg {
                    info!("Processing job: {:?}", job.job_id);

                    // Create completion channel for this job
                    let (completion_tx, completion_rx) = oneshot::channel();
                    let cancel_token = CancellationToken::new();

                    // Register job for cancellation tracking
                    let job_handle = tokio::spawn(async move {
                        // This is a placeholder for the actual job execution
                        // The real implementation would be more complex
                    });

                    job_registry.register_job(
                        job.job_execution_id.clone(),
                        job.job_document.clone(),
                        cancel_token.clone(),
                        job_handle,
                    ).await;

                    // Start job execution
                    let job_clone = job.clone();
                    let job_client_clone = job_client.clone();
                    let handlers_clone = handlers.clone();
                    let job_registry_clone = job_registry.clone();

                    tokio::spawn(async move {
                        let result = handlers_clone.handle_job_execution(
                            &job_clone,
                            &job_client_clone,
                            completion_tx,
                            cancel_token,
                        ).await;

                        if let Err(e) = result {
                            error!("Job execution setup failed: {:?}", e);
                        }
                    });

                    // Wait for job completion in a separate task
                    let job_client_for_completion = job_client.clone();
                    let job_execution_id = job.job_execution_id.clone();
                    tokio::spawn(async move {
                        match completion_rx.await {
                            Ok(completion) => {
                                info!("Job {} completed with status: {:?}", job_execution_id, completion.status);

                                // Unregister job
                                job_registry_clone.unregister_job(&job_execution_id).await;

                                // Request next job
                                if let Err(e) = job_client_for_completion.request_next_job().await {
                                    error!("Failed to request next job: {:?}", e);
                                }
                            }
                            Err(e) => {
                                error!("Job completion channel error: {:?}", e);

                                // Unregister job
                                job_registry_clone.unregister_job(&job_execution_id).await;

                                // Still try to request next job
                                if let Err(e) = job_client_for_completion.request_next_job().await {
                                    error!("Failed to request next job: {:?}", e);
                                }
                            }
                        }
                    });
                }
            }
        }
    }

    info!("Shutting down fleet commander completed");
    Ok(())
}
