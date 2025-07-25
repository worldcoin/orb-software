use std::process::Stdio;

use color_eyre::eyre::{eyre, Error, Result};
use orb_relay_messages::jobs::v1::{
    JobExecution, JobExecutionStatus, JobExecutionUpdate,
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    sync::oneshot,
};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use crate::job_client::JobClient;
use crate::orchestrator::JobCompletion;

#[derive(Debug, Clone)]
pub struct TailLogsCommandHandler;

impl TailLogsCommandHandler {
    pub fn new() -> Self {
        Self
    }

    #[tracing::instrument]
    pub async fn handle_logs(
        &self,
        job: &JobExecution,
        job_client: &JobClient,
        completion_tx: oneshot::Sender<JobCompletion>,
        cancel_token: CancellationToken,
        log_source: &str,
    ) -> Result<(), Error> {
        info!(
            "Tailing logs: {} for job {}",
            log_source, job.job_execution_id
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
                error!("Failed to send cancellation update: {:?}", e);
            }
            completion_tx
                .send(JobCompletion::cancelled(job.job_execution_id.clone()))
                .ok();
            return Ok(());
        }

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
            completion_tx
                .send(JobCompletion::failure(
                    job.job_execution_id.clone(),
                    format!("{:?}", e),
                ))
                .ok();
            return Ok(());
        }

        // Spawn background task for log tailing
        let job_clone = job.clone();
        let job_client_clone = job_client.clone();
        let log_source_clone = log_source.to_string();

        tokio::spawn(async move {
            let result = if log_source_clone == "test" {
                tail_logs_test(&job_clone, &job_client_clone, &cancel_token).await
            } else {
                tail_logs_from_process(
                    &job_clone,
                    &job_client_clone,
                    &log_source_clone,
                    &cancel_token,
                )
                .await
            };

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

async fn tail_logs<R>(
    job: &JobExecution,
    job_client: &JobClient,
    reader: &mut BufReader<R>,
    cancel: &CancellationToken,
) -> Result<(), Error>
where
    R: tokio::io::AsyncRead + Unpin,
{
    // Set up a timer to automatically cancel after 5 minutes
    let cancel_clone = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_secs(5 * 60)).await;
        cancel_clone.cancel();
    });

    loop {
        let mut line = String::new();
        tokio::select! {
            _ = cancel.cancelled() => {
                let job_update = JobExecutionUpdate {
                    job_id: job.job_id.clone(),
                    job_execution_id: job.job_execution_id.clone(),
                    status: JobExecutionStatus::Succeeded as i32,
                    std_out: String::new(),
                    std_err: "cancelled".to_string(),
                };
                if let Err(e) = job_client.send_job_update(&job_update).await {
                    error!("Failed to send job update: {:?}", e);
                }
                break Ok(());
            }
            read_result = reader.read_line(&mut line) => {
                let job_update = match read_result {
                    Ok(0) => JobExecutionUpdate {
                        job_id: job.job_id.clone(),
                        job_execution_id: job.job_execution_id.clone(),
                        status: JobExecutionStatus::Succeeded as i32,
                        std_out: String::new(),
                        std_err: String::new(),
                    },
                    Ok(_num_bytes) => JobExecutionUpdate {
                        job_id: job.job_id.clone(),
                        job_execution_id: job.job_execution_id.clone(),
                        status: JobExecutionStatus::InProgress as i32,
                        std_out: line.clone(),
                        std_err: String::new(),
                    },
                    Err(e) => JobExecutionUpdate {
                        job_id: job.job_id.clone(),
                        job_execution_id: job.job_execution_id.clone(),
                        status: JobExecutionStatus::Failed as i32,
                        std_out: String::new(),
                        std_err: format!("Failed to read line: {:?}", e),
                    },
                };

                if let Err(e) = job_client.send_job_update(&job_update).await {
                    let msg = format!("Failed to send job update: {:?}", e);
                    error!("{}", msg);
                    break Err(eyre!(msg));
                }
                if job_update.status != JobExecutionStatus::InProgress as i32 {
                    break Ok(());
                }
            }
        }
    }
}

async fn tail_logs_from_process(
    job: &JobExecution,
    job_client: &JobClient,
    log_source: &str,
    cancel: &CancellationToken,
) -> Result<(), Error> {
    let output = match tokio::process::Command::new("sudo")
        .args(vec!["journalctl", "-f", "-u", log_source])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(output) => output,
        Err(e) => {
            let msg = format!("Failed to tail logs: {:?}", e);
            error!("{}", msg);
            let update = JobExecutionUpdate {
                job_id: job.job_id.clone(),
                job_execution_id: job.job_execution_id.clone(),
                status: JobExecutionStatus::Failed as i32,
                std_out: String::new(),
                std_err: msg.clone(),
            };
            let _ = job_client.send_job_update(&update).await;
            return Err(eyre!(msg));
        }
    };

    let out = output
        .stdout
        .ok_or_else(|| eyre!("Failed to get stdout from journalctl"))?;
    let mut reader = tokio::io::BufReader::new(out);
    tail_logs(job, job_client, &mut reader, cancel).await
}

async fn tail_logs_test(
    job: &JobExecution,
    job_client: &JobClient,
    cancel: &CancellationToken,
) -> Result<(), Error> {
    // Create a cursor over a byte slice for testing
    let test_logs = b"Test log line 1\nTest log line 2\n";
    let mut reader = BufReader::new(&test_logs[..]);
    tail_logs(job, job_client, &mut reader, cancel).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers_old::tests::{create_test_client, create_test_server};
    use crate::orchestrator::JobRegistry;
    use orb_relay_messages::relay::entity::EntityType;

    #[tokio::test]
    async fn test_tail_logs() {
        // Arrange
        let sv = create_test_server().await;
        let _client_svc =
            create_test_client("test_svc", "test_namespace", EntityType::Service, &sv)
                .await;
        let client_orb =
            create_test_client("test_orb", "test_namespace", EntityType::Orb, &sv)
                .await;
        let job_client = JobClient::new(
            client_orb,
            "test_svc",
            "test_namespace",
            JobRegistry::new(),
            crate::orchestrator::JobConfig::new(),
        );

        let job = JobExecution {
            job_id: "test_job_id".to_string(),
            job_execution_id: "test_job_execution_id".to_string(),
            job_document: "tail_test".to_string(),
            should_cancel: false,
        };

        let handler = TailLogsCommandHandler::new();
        let (completion_tx, completion_rx) = oneshot::channel();
        let cancel_token = CancellationToken::new();

        // Act
        let handle_result = handler
            .handle_logs(&job, &job_client, completion_tx, cancel_token, "test")
            .await;
        assert!(handle_result.is_ok());

        // Wait for completion
        let completion = completion_rx.await.unwrap();
        assert_eq!(completion.status, JobExecutionStatus::Succeeded);
        assert_eq!(completion.job_execution_id, "test_job_execution_id");
    }
}
