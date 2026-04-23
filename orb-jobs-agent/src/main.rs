use clap::Parser;
use color_eyre::eyre::{eyre, Context, ContextCompat, Result};
use orb_endpoints::{v1::Endpoints, Backend};
use orb_info::TokenTaskHandle;
use orb_jobs_agent::settings::Settings;
use orb_jobs_agent::shell::Host;
use orb_jobs_agent::{
    args::Args,
    job_system::client::{JobTransport, LocalTransport, RelayTransport},
    program::{self, Deps, Runtime},
};
use orb_relay_client::{Auth, Client, ClientOpts};
use orb_relay_messages::{
    jobs::v1::{JobExecution, JobExecutionStatus},
    relay::entity::EntityType,
};
use std::{sync::Arc, time::Duration};
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
        Some(job_document) => run_local(deps, job_document).await?,
        None => run_service(deps, args, &settings).await?,
    }

    info!("Shutting down jobs agent completed");
    Ok(())
}

async fn run_local(deps: Deps, job_document: &str) -> Result<()> {
    let job = JobExecution {
        job_id: "local-job".to_string(),
        job_execution_id: "local-job-execution".to_string(),
        job_document: job_document.to_string(),
        should_cancel: false,
    };

    let transport = Arc::new(LocalTransport::new(job));
    let runtime = Runtime {
        transport: transport.clone(),
        transport_handle: transport.shutdown_handle(),
        watch_conn_changes: false,
    };

    program::run(deps, runtime).await?;

    let terminal_update = transport
        .terminal_update()
        .ok_or_else(|| eyre!("local run ended without terminal job status"))?;

    if terminal_update.status != JobExecutionStatus::Succeeded as i32 {
        let status_name = JobExecutionStatus::try_from(terminal_update.status)
            .map(|status| format!("{status:?}"))
            .unwrap_or_else(|_| format!("Unknown({})", terminal_update.status));

        if terminal_update.std_err.is_empty() {
            return Err(eyre!("local job failed with status {status_name}"));
        }

        return Err(eyre!(
            "local job failed with status {status_name}: {}",
            terminal_update.std_err
        ));
    }

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

    let auth = resolve_auth(args, &deps.session_dbus).await?;

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
    let (relay_client, transport_handle) = Client::connect(opts);
    let transport: Arc<dyn JobTransport> = Arc::new(RelayTransport::new(
        relay_client,
        target_service_id,
        relay_namespace,
    ));
    let runtime = Runtime {
        transport,
        transport_handle,
        watch_conn_changes: true,
    };

    program::run(deps, runtime).await
}

async fn resolve_auth(args: &Args, connection: &zbus::Connection) -> Result<Auth> {
    match &args.orb_token {
        Some(token) => Ok(Auth::Token(token.as_str().into())),
        None => {
            let shutdown = CancellationToken::new();
            let get_token = async || {
                TokenTaskHandle::spawn(connection, &shutdown)
                    .await
                    .wrap_err("failed to get auth token!")
            };

            let token_recv = tokio::time::timeout(Duration::from_secs(60), async {
                loop {
                    match get_token().await {
                        Ok(handle) => return handle.token_recv,
                        Err(e) => {
                            warn!("{e}! trying again in 5s");
                            tokio::time::sleep(Duration::from_secs(5)).await;
                        }
                    }
                }
            })
            .await
            .wrap_err("could not get auth token after 60s")?;

            Ok(Auth::TokenReceiver(token_recv))
        }
    }
}
