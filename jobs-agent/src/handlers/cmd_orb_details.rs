use color_eyre::eyre::{Error, Result};
use orb_info::{OrbJabilId, OrbName};
use orb_relay_messages::jobs::v1::{
    JobExecution, JobExecutionStatus, JobExecutionUpdate,
};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use crate::job_client::JobClient;
use crate::orchestrator::JobCompletion;

#[derive(Debug, Clone)]
pub struct OrbDetailsCommandHandler {}

impl OrbDetailsCommandHandler {
    pub fn new() -> Self {
        Self {}
    }

    #[tracing::instrument]
    pub async fn handle(
        &self,
        job: &JobExecution,
        job_client: &JobClient,
        completion_tx: oneshot::Sender<JobCompletion>,
        cancel_token: CancellationToken,
    ) -> Result<(), Error> {
        info!(
            "Handling orb details command for job {}",
            job.job_execution_id
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

        // Execute the orb details logic
        let orb_name = OrbName::read()
            .await
            .unwrap_or(OrbName("NO_ORB_NAME".to_string()));
        let jabil_id = OrbJabilId::read()
            .await
            .unwrap_or(OrbJabilId("NO_JABIL_ID".to_string()));

        let details = serde_json::json!({
            "orb_name": orb_name.to_string(),
            "jabil_id": jabil_id.to_string(),
        });

        let update = JobExecutionUpdate {
            job_id: job.job_id.clone(),
            job_execution_id: job.job_execution_id.clone(),
            status: JobExecutionStatus::Succeeded as i32,
            std_out: details.to_string(),
            std_err: String::new(),
        };

        // Send the job update
        if let Err(e) = job_client.send_job_update(&update).await {
            error!("Failed to send job update: {:?}", e);
        }

        // Signal completion
        completion_tx
            .send(JobCompletion::success(job.job_execution_id.clone()))
            .ok();

        Ok(())
    }
}
