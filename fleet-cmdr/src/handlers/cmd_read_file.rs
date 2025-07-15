use color_eyre::eyre::{Error, Result};
use orb_relay_messages::fleet_cmdr::v1::{
    JobExecution, JobExecutionStatus, JobExecutionUpdate,
};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use crate::job_client::JobClient;
use crate::orchestrator::JobCompletion;

#[derive(Debug, Clone)]
pub struct ReadFileCommandHandler {}

impl ReadFileCommandHandler {
    pub fn new() -> Self {
        Self {}
    }

    #[tracing::instrument]
    pub async fn handle_file(
        &self,
        job: &JobExecution,
        job_client: &JobClient,
        completion_tx: oneshot::Sender<JobCompletion>,
        cancel_token: CancellationToken,
        file_path: &str,
    ) -> Result<(), Error> {
        info!(
            "Reading file: {} for job {}",
            file_path, job.job_execution_id
        );

        // Check for cancellation before starting
        if cancel_token.is_cancelled() {
            let update = JobExecutionUpdate {
                job_id: job.job_id.clone(),
                job_execution_id: job.job_execution_id.clone(),
                status: JobExecutionStatus::Failed as i32,
                std_out: String::new(),
                std_err: "Job was cancelled".to_string(),
            };

            if let Err(e) = job_client.send_job_update(&update).await {
                error!("Failed to send job update: {:?}", e);
            }
            completion_tx
                .send(JobCompletion::cancelled(job.job_execution_id.clone()))
                .ok();
            return Ok(());
        }

        // Execute the file reading logic
        let result = tokio::fs::read(file_path).await;

        let update = match result {
            Ok(content) => JobExecutionUpdate {
                job_id: job.job_id.clone(),
                job_execution_id: job.job_execution_id.clone(),
                status: JobExecutionStatus::Succeeded as i32,
                std_out: String::from_utf8_lossy(&content).to_string(),
                std_err: String::new(),
            },
            Err(e) => {
                error!("Failed to read file '{}': {:?}", file_path, e);
                JobExecutionUpdate {
                    job_id: job.job_id.clone(),
                    job_execution_id: job.job_execution_id.clone(),
                    status: JobExecutionStatus::Failed as i32,
                    std_out: String::new(),
                    std_err: e.to_string(),
                }
            }
        };

        // Send the job update
        if let Err(e) = job_client.send_job_update(&update).await {
            error!("Failed to send job update: {:?}", e);
        }

        // Signal completion
        let completion = if update.status == JobExecutionStatus::Succeeded as i32 {
            JobCompletion::success(job.job_execution_id.clone())
        } else {
            JobCompletion::failure(job.job_execution_id.clone(), update.std_err.clone())
        };

        completion_tx.send(completion).ok();

        Ok(())
    }
}
