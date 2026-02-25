use clap::Parser;
use color_eyre::eyre::{eyre, Context, ContextCompat, Result};
use orb_endpoints::{v1::Endpoints, Backend};
use orb_info::TokenTaskHandle;
use orb_jobs_agent::args::Args;
use orb_jobs_agent::job_system::client::{LocalTransport, RelayTransport};
use orb_jobs_agent::program::{self, Deps};
use orb_jobs_agent::settings::Settings;
use orb_jobs_agent::shell::Host;
use orb_relay_client::{Auth, Client, ClientOpts};
use orb_relay_messages::jobs::v1::{JobExecution, JobExecutionStatus};
use orb_relay_messages::relay::entity::EntityType;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

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

    let connection = zbus::ConnectionBuilder::address(args.dbus_addr.as_str())?
        .build()
        .await?;

    let settings = Settings::from_args(args, "/mnt/scratch").await?;

    let deps = Deps::new(Host, connection, settings.clone());

    match &args.run_job {
        Some(job_document) => run_local(deps, job_document).await,
        None => run_service(deps, args, &settings).await,
    }
}

async fn run_local(deps: Deps, job_document: &str) -> Result<()> {
    let job = JobExecution {
        job_id: "local-job".to_string(),
        job_execution_id: "local-job-execution".to_string(),
        job_document: job_document.to_string(),
        should_cancel: false,
    };

    let (local_transport, _shutdown_token) = LocalTransport::new(job);
    let transport = Arc::new(local_transport);
    let relay_handle = {
        let t = Arc::clone(&transport);
        t.shutdown_handle()
    };

    program::run(deps, Arc::clone(&transport) as _, relay_handle).await?;

    let status = transport
        .final_status()
        .ok_or_else(|| eyre!("local run ended without terminal job status"))?;

    if status != JobExecutionStatus::Succeeded as i32 {
        let status_name = JobExecutionStatus::try_from(status)
            .map(|s| format!("{s:?}"))
            .unwrap_or_else(|_| format!("Unknown({status})"));

        return Err(eyre!("local job failed with status {status_name}"));
    }

    info!("Shutting down jobs agent completed");

    Ok(())
}

async fn run_service(deps: Deps, args: &Args, settings: &Settings) -> Result<()> {
    let relay_host = args
        .relay_host
        .clone()
        .or_else(|| {
            Backend::from_env().ok().map(|backend| {
                Endpoints::new(backend, &settings.orb_id).relay.to_string()
            })
        })
        .wrap_err("could not get Backend Endpoint from env")?;

    let auth = match &args.orb_token {
        Some(t) => Auth::Token(t.as_str().into()),
        None => {
            let shutdown_token = CancellationToken::new();
            let dbus_addr = args.dbus_addr.clone();
            let get_token = async || {
                let connection = zbus::ConnectionBuilder::address(dbus_addr.as_str())?
                    .build()
                    .await
                    .map_err(|e| {
                        eyre!("failed to establish zbus conn at {}: {e}", dbus_addr)
                    })?;

                TokenTaskHandle::spawn(&connection, &shutdown_token)
                    .await
                    .wrap_err("failed to get auth token!")
            };

            let token_rec_fut = async {
                loop {
                    match get_token().await {
                        Err(e) => {
                            warn!("{e}! trying again in 5s");
                            tokio::time::sleep(Duration::from_secs(5)).await;
                            continue;
                        }
                        Ok(t) => break t.token_recv,
                    }
                }
            };

            let token_rec =
                tokio::time::timeout(Duration::from_secs(60), token_rec_fut)
                    .await
                    .wrap_err("could not get auth token after 60s")?;

            Auth::TokenReceiver(token_rec)
        }
    };

    let relay_namespace = args
        .relay_namespace
        .clone()
        .wrap_err("relay namespace MUST be provided")?;

    let target_service_id = args
        .target_service_id
        .clone()
        .wrap_err("target service id MUST be provided")?;

    let opts = ClientOpts::entity(EntityType::Orb)
        .id(settings.orb_id.as_str().to_string())
        .endpoint(&relay_host)
        .namespace(&relay_namespace)
        .auth(auth)
        .connection_timeout(Duration::from_secs(3))
        .connection_backoff(Duration::from_secs(2))
        .keep_alive_interval(Duration::from_secs(30))
        .keep_alive_timeout(Duration::from_secs(10))
        .ack_timeout(Duration::from_secs(5))
        .build();

    info!("Connecting to relay: {:?}", relay_host);
    let (relay_client, relay_handle) = Client::connect(opts);
    let transport = Arc::new(RelayTransport::new(
        relay_client,
        target_service_id,
        relay_namespace,
    ));

    program::run(deps, transport, relay_handle).await?;

    info!("Shutting down jobs agent completed");

    Ok(())
}
