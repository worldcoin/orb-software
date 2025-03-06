use color_eyre::eyre::{eyre, Error, Result};
use orb_relay_messages::fleet_cmdr::v1::{
    JobExecution, JobExecutionStatus, JobExecutionUpdate,
};
use tracing::{error, info};

use crate::job_client::JobClient;

#[derive(Debug)]
pub struct ReadFileCommandHandler {}

impl ReadFileCommandHandler {
    pub fn new() -> Self {
        Self {}
    }
}

impl ReadFileCommandHandler {
    #[tracing::instrument]
    pub async fn handle(
        &self,
        job: &JobExecution,
        job_client: &JobClient,
        file_path: &str,
    ) -> Result<JobExecutionUpdate, Error> {
        info!("Reading file: {}", file_path);
        match tokio::fs::read(file_path).await {
            Ok(content) => Ok(JobExecutionUpdate {
                job_id: job.job_id.clone(),
                job_execution_id: job.job_execution_id.clone(),
                status: JobExecutionStatus::Succeeded as i32,
                std_out: String::from_utf8_lossy(&content).to_string(),
                std_err: String::new(),
            }),
            Err(e) => {
                error!("Failed to read file '{}': {:?}", file_path, e);
                Err(eyre!(e))
            }
        }
    }
}
