use color_eyre::eyre::{eyre, Error, Result};
use orb_relay_messages::fleet_cmdr::v1::{
    JobExecution, JobExecutionStatus, JobExecutionUpdate,
};
use tracing::{error, info};

use crate::job_client::JobClient;

#[derive(Debug)]
pub struct RunBinaryCommandHandler {}

impl RunBinaryCommandHandler {
    pub fn new() -> Self {
        Self {}
    }
}

impl RunBinaryCommandHandler {
    #[tracing::instrument]
    pub async fn handle(
        &self,
        job: &JobExecution,
        job_client: &JobClient,
        bin: &str,
        args: &Vec<String>,
    ) -> Result<JobExecutionUpdate, Error> {
        info!("Running binary: {}", bin);
        let job_clone = job.clone();
        let job_client_clone = job_client.clone();
        let bin_clone = bin.to_string();
        let args_clone = args.clone();

        let _handle = tokio::task::spawn(async move {
            let _ = run_binary(&job_clone, &job_client_clone, &bin_clone, &args_clone)
                .await;
        });

        Ok(JobExecutionUpdate {
            job_id: job.job_id.clone(),
            job_execution_id: job.job_execution_id.clone(),
            status: JobExecutionStatus::InProgress as i32,
            std_out: String::new(),
            std_err: String::new(),
        })
    }
}

async fn run_binary(
    job: &JobExecution,
    job_client: &JobClient,
    bin: &str,
    args: &Vec<String>,
) -> Result<(), Error> {
    let output = match tokio::process::Command::new(bin).args(args).output().await {
        Ok(output) => output,
        Err(e) => {
            let msg = format!("Failed to run binary '{}': {:?}", bin, e);
            error!("{}", msg.clone());
            let _ = job_client
                .send_job_update(&JobExecutionUpdate {
                    job_id: job.job_id.clone(),
                    job_execution_id: job.job_execution_id.clone(),
                    status: JobExecutionStatus::Failed as i32,
                    std_out: String::new(),
                    std_err: msg.clone(),
                })
                .await;
            return Err(eyre!(msg));
        }
    };

    let job_update = JobExecutionUpdate {
        job_id: job.job_id.clone(),
        job_execution_id: job.job_execution_id.clone(),
        status: JobExecutionStatus::Succeeded as i32,
        std_out: String::from_utf8_lossy(&output.stdout).to_string(),
        std_err: String::from_utf8_lossy(&output.stderr).to_string(),
    };

    match job_client.send_job_update(&job_update).await {
        Ok(_) => Ok(()),
        Err(e) => {
            error!("Failed to send job update: {:?}", e);
            Err(eyre!("Failed to send job update: {:?}", e))
        }
    }
}
