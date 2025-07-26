use crate::job_system::client::JobClient;
use crate::job_system::orchestrator::JobCompletion;
use color_eyre::eyre::{eyre, Error, Result};
use orb_relay_messages::jobs::v1::{
    JobExecution, JobExecutionStatus, JobExecutionUpdate,
};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

#[derive(Debug, Clone)]
pub struct RunBinaryCommandHandler {}

impl RunBinaryCommandHandler {
    pub fn new() -> Self {
        Self {}
    }

    #[tracing::instrument]
    pub async fn handle_binary(
        &self,
        job: &JobExecution,
        job_client: &JobClient,
        completion_tx: oneshot::Sender<JobCompletion>,
        cancel_token: CancellationToken,
        bin: &str,
        args: &Vec<String>,
    ) -> Result<(), Error> {
        info!("Running binary: {} for job {}", bin, job.job_execution_id);

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

        // Spawn background task to run the binary
        let job_clone = job.clone();
        let job_client_clone = job_client.clone();
        let bin_clone = bin.to_string();
        let args_clone = args.clone();

        tokio::spawn(async move {
            let result = run_binary(
                &job_clone,
                &job_client_clone,
                &bin_clone,
                &args_clone,
                &cancel_token,
            )
            .await;

            let completion = match result {
                Ok(()) => JobCompletion::success(job_clone.job_execution_id.clone()),
                Err(e) => JobCompletion::failure(
                    job_clone.job_execution_id.clone(),
                    e.to_string(),
                ),
            };

            completion_tx.send(completion).ok();
        });

        Ok(())
    }
}

async fn run_binary(
    job: &JobExecution,
    job_client: &JobClient,
    bin: &str,
    args: &Vec<String>,
    cancel_token: &CancellationToken,
) -> Result<(), Error> {
    // Send initial progress update
    let progress_update = JobExecutionUpdate {
        job_id: job.job_id.clone(),
        job_execution_id: job.job_execution_id.clone(),
        status: JobExecutionStatus::InProgress as i32,
        std_out: String::new(),
        std_err: String::new(),
    };

    if let Err(e) = job_client.send_job_update(&progress_update).await {
        error!("Failed to send progress update: {:?}", e);
        return Err(eyre!("Failed to send progress update: {:?}", e));
    }

    // Check for cancellation before starting the command
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
        return Err(eyre!("Job was cancelled"));
    }

    // Execute the binary
    let output = match tokio::process::Command::new(bin).args(args).output().await {
        Ok(output) => output,
        Err(e) => {
            let msg = format!("Failed to run binary '{}': {:?}", bin, e);
            error!("{}", msg);

            let update = JobExecutionUpdate {
                job_id: job.job_id.clone(),
                job_execution_id: job.job_execution_id.clone(),
                status: JobExecutionStatus::Failed as i32,
                std_out: String::new(),
                std_err: msg.clone(),
            };

            if let Err(e) = job_client.send_job_update(&update).await {
                error!("Failed to send job update: {:?}", e);
            }
            return Err(eyre!(msg));
        }
    };

    // Send final result
    let job_update = JobExecutionUpdate {
        job_id: job.job_id.clone(),
        job_execution_id: job.job_execution_id.clone(),
        status: JobExecutionStatus::Succeeded as i32,
        std_out: String::from_utf8_lossy(&output.stdout).to_string(),
        std_err: String::from_utf8_lossy(&output.stderr).to_string(),
    };

    if let Err(e) = job_client.send_job_update(&job_update).await {
        error!("Failed to send job update: {:?}", e);
        return Err(eyre!("Failed to send job update: {:?}", e));
    }
    Ok(())
}
