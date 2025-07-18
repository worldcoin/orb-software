use std::str::FromStr;

use clap::Parser;
use color_eyre::eyre::Result;
use orb_endpoints::{backend::Backend, v1::Endpoints};
use orb_info::{OrbId, TokenTaskHandle};
use orb_jobs_agent::{
    args::Args,
    handlers::OrbCommandHandlers,
    job_client::JobClient,
    orchestrator::{JobConfig, JobRegistry},
};
use orb_relay_client::{Auth, Client, ClientOpts};
use orb_relay_messages::jobs::v1::{JobExecutionStatus, JobExecutionUpdate};
use orb_relay_messages::relay::entity::EntityType;
use tokio::sync::{oneshot, watch};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};
use zbus::Connection;

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

    let orb_id = if let Some(id) = &args.orb_id {
        OrbId::from_str(id)?
    } else {
        OrbId::read().await?
    };
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
    let job_config = JobConfig::new();
    let handlers = OrbCommandHandlers::init().await;

    // Init Relay Client
    info!("Connecting to relay: {:?}", endpoints);
    let opts = ClientOpts::entity(EntityType::Orb)
        .id(orb_id.as_str().to_string())
        .endpoint(endpoints.clone())
        .namespace(args.relay_namespace.clone().unwrap())
        .auth(Auth::TokenReceiver(auth_token))
        .build();
    let (relay_client, mut relay_handle) = Client::connect(opts);
    let job_client = JobClient::new(
        relay_client.clone(),
        args.target_service_id.clone().unwrap().as_str(),
        args.relay_namespace.clone().unwrap().as_str(),
        job_registry.clone(),
        job_config.clone(),
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
                info!("Shutting down jobs agent initiated");
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
                match job_client.try_request_more_jobs().await {
                    Ok(true) => {
                        info!("Successfully requested initial job");
                    }
                    Ok(false) => {
                        // No jobs available, try basic request
                        if let Err(e) = job_client.request_next_job().await {
                            error!("Failed to request initial job: {:?}", e);
                        }
                    }
                    Err(e) => {
                        error!("Failed to request initial job via parallel logic: {:?}, trying basic request", e);
                        if let Err(e) = job_client.request_next_job().await {
                            error!("Failed to request initial job: {:?}", e);
                        }
                    }
                }
            }
            msg = job_client.listen_for_job() => {
                if let Ok(job) = msg {
                    info!("Processing job: {:?}", job.job_id);

                    // Check if job is already cancelled
                    if job.should_cancel {
                        info!("Job {} is already marked for cancellation, acknowledging and skipping execution", job.job_execution_id);

                        // Send cancellation acknowledgment
                        let cancel_update = JobExecutionUpdate {
                            job_id: job.job_id.clone(),
                            job_execution_id: job.job_execution_id.clone(),
                            status: JobExecutionStatus::Cancelled as i32,
                            std_out: String::new(),
                            std_err: String::new(),
                        };

                        if let Err(e) = job_client.send_job_update(&cancel_update).await {
                            error!("Failed to send cancellation acknowledgment: {:?}", e);
                        }

                        // Request next job immediately after cancellation acknowledgment
                        match job_client.try_request_more_jobs().await {
                            Ok(true) => {
                                info!("Successfully requested job after cancellation acknowledgment");
                            }
                            Ok(false) => {
                                // No more jobs or at limits, try basic request
                                if let Err(e) = job_client.request_next_job().await {
                                    error!("Failed to request next job after cancellation acknowledgment: {:?}", e);
                                }
                            }
                            Err(e) => {
                                error!("Failed to request job via parallel logic after cancellation: {:?}, trying basic request", e);
                                if let Err(e) = job_client.request_next_job().await {
                                    error!("Failed to request next job after cancellation acknowledgment: {:?}", e);
                                }
                            }
                        }

                        continue;
                    }

                    // Check if this job can be started based on parallelization rules
                    let job_type = job.job_document.clone();
                    if !job_config.can_start_job(&job_type, &job_registry).await {
                        info!("Job '{}' of type '{}' cannot be started due to parallelization constraints, skipping",
                              job.job_execution_id, job_type);

                        // Send a message indicating we're skipping this job and request another
                        match job_client.try_request_more_jobs().await {
                            Ok(true) => {
                                info!("Requested alternative job after skipping incompatible job");
                            }
                            Ok(false) => {
                                if let Err(e) = job_client.request_next_job().await {
                                    error!("Failed to request next job after skipping: {:?}", e);
                                }
                            }
                            Err(e) => {
                                error!("Failed to request job after skipping: {:?}", e);
                            }
                        }
                        continue;
                    }

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

                    // Check if this job supports parallel execution and request more jobs if appropriate
                    if job_config.is_parallel(&job_type) {
                        info!("Started parallel job '{}', checking for additional jobs", job_type);

                        // Try to request more jobs for parallel execution
                        match job_client.try_request_more_jobs().await {
                            Ok(true) => {
                                info!("Successfully requested additional job for parallel execution");
                            }
                            Ok(false) => {
                                info!("No additional jobs requested (at parallelization limits or no jobs available)");
                            }
                            Err(e) => {
                                error!("Failed to request additional job for parallel execution: {:?}", e);
                            }
                        }
                    } else if job_config.is_sequential(&job_type) {
                        info!("Started sequential job '{}', will not request additional jobs", job_type);
                    }

                    // Wait for job completion in a separate task
                    let job_client_for_completion = job_client.clone();
                    let job_execution_id = job.job_execution_id.clone();
                    tokio::spawn(async move {
                        match completion_rx.await {
                            Ok(completion) => {
                                info!("Job {} completed with status: {:?}", job_execution_id, completion.status);

                                // Unregister job
                                job_registry_clone.unregister_job(&job_execution_id).await;

                                // Try to request more jobs for parallel execution
                                match job_client_for_completion.try_request_more_jobs().await {
                                    Ok(true) => {
                                        info!("Requested additional job after job completion");
                                    }
                                    Ok(false) => {
                                        // No more jobs available or at limits, just request next job normally
                                        if let Err(e) = job_client_for_completion.request_next_job().await {
                                            error!("Failed to request next job: {:?}", e);
                                        }
                                    }
                                    Err(e) => {
                                        error!("Failed to request additional job: {:?}, trying normal request", e);
                                        // Fallback to normal job request
                                        if let Err(e) = job_client_for_completion.request_next_job().await {
                                            error!("Failed to request next job: {:?}", e);
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                error!("Job completion channel error: {:?}", e);

                                // Unregister job
                                job_registry_clone.unregister_job(&job_execution_id).await;

                                // Still try to request more jobs
                                match job_client_for_completion.try_request_more_jobs().await {
                                    Ok(_) => {}
                                    Err(e) => {
                                        error!("Failed to request additional job after error: {:?}, trying normal request", e);
                                        if let Err(e) = job_client_for_completion.request_next_job().await {
                                            error!("Failed to request next job: {:?}", e);
                                        }
                                    }
                                }
                            }
                        }
                    });
                }
            }
        }
    }

    info!("Shutting down jobs agent completed");
    Ok(())
}
