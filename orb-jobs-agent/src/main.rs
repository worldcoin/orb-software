use clap::Parser;
use color_eyre::eyre::Result;
use orb_jobs_agent::args::Args;
use orb_jobs_agent::program::{self, Deps, JobMode};
use orb_jobs_agent::settings::Settings;
use orb_jobs_agent::shell::Host;
use orb_relay_messages::jobs::v1::JobExecution;
use tracing::info;

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
    let job_mode = match &args.run_job {
        Some(job_document) => JobMode::LocalSingleJob(JobExecution {
            job_id: "local-job".to_string(),
            job_execution_id: "local-job-execution".to_string(),
            job_document: job_document.clone(),
            should_cancel: false,
        }),
        None => JobMode::Service,
    };

    let deps = Deps::new(
        Host,
        connection,
        Settings::from_args(args, "/mnt/scratch").await?,
        job_mode,
    );

    program::run(deps).await?;

    info!("Shutting down jobs agent completed");
    Ok(())
}
