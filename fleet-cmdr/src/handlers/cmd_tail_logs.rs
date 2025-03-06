use std::{process::Stdio, sync::Arc};

use color_eyre::eyre::{eyre, Error, Result};
use orb_relay_messages::fleet_cmdr::v1::{
    JobExecution, JobExecutionStatus, JobExecutionUpdate,
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use crate::job_client::JobClient;

#[derive(Debug, Clone)]
pub struct TailLogsCommandHandler {
    handle: Option<Arc<JoinHandle<Result<(), Error>>>>,
    cancel: Option<CancellationToken>,
}

impl TailLogsCommandHandler {
    pub fn new() -> Self {
        Self {
            handle: None,
            cancel: None,
        }
    }
}

impl TailLogsCommandHandler {
    #[tracing::instrument]
    pub async fn handle(
        &mut self,
        job: &JobExecution,
        job_client: &JobClient,
        log_source: &str,
    ) -> Result<JobExecutionUpdate, Error> {
        info!("Tailing logs: {}", log_source);
        let job_clone = job.clone();
        let job_client_clone = job_client.clone();
        let log_source_clone = log_source.to_string();

        if let Some(cancel) = self.cancel.as_ref() {
            cancel.cancel();
        }

        let cancel = CancellationToken::new();
        self.cancel = Some(cancel.clone());
        self.handle = Some(Arc::new(tokio::task::spawn(async move {
            if log_source_clone == "test" {
                tail_logs_test(&job_clone, &job_client_clone, &cancel).await
            } else {
                tail_logs_from_process(
                    &job_clone,
                    &job_client_clone,
                    &log_source_clone,
                    &cancel,
                )
                .await
            }
        })));

        Ok(JobExecutionUpdate {
            job_id: job.job_id.clone(),
            job_execution_id: job.job_execution_id.clone(),
            status: JobExecutionStatus::InProgress as i32,
            std_out: String::new(),
            std_err: String::new(),
        })
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
                    error!("{}", msg.clone());
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

    let out = output.stdout.unwrap();
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
    use crate::handlers::tests::{create_test_client, create_test_server};
    use orb_relay_messages::{
        fleet_cmdr::v1::JobExecutionUpdate, prost::Message, prost_types::Any,
        relay::entity::EntityType,
    };
    use tokio::task;

    #[tokio::test]
    async fn test_tail_logs() {
        // Arrange
        let sv = create_test_server().await;
        let client_svc =
            create_test_client("test_svc", "test_namespace", EntityType::Service, &sv)
                .await;
        let client_orb =
            create_test_client("test_orb", "test_namespace", EntityType::Orb, &sv)
                .await;
        let job_client = JobClient::new(client_orb, "test_svc", "test_namespace");

        let job = JobExecution {
            job_id: "test_job_id".to_string(),
            job_execution_id: "test_job_execution_id".to_string(),
            job_document: "tail_test".to_string(),
        };

        let mut handler = TailLogsCommandHandler::new();

        // Act
        let handle_result = handler.handle(&job, &job_client, "test").await;
        assert!(handle_result.is_ok());

        // Spawn a task to receive and verify the updates
        let received_updates = task::spawn(async move {
            let mut updates = Vec::new();

            // We expect at least 3 updates:
            // 1. Initial InProgress update
            // 2. Update with "Test log line 1"
            // 3. Update with "Test log line 2"
            for _ in 0..3 {
                if let Ok(msg) = client_svc.recv().await {
                    let any = Any::decode(msg.payload.as_slice()).unwrap();
                    let update =
                        JobExecutionUpdate::decode(any.value.as_slice()).unwrap();
                    updates.push(update);
                }
            }

            updates
        });

        // Wait for the task to complete
        let updates = received_updates.await.unwrap();

        // Assert
        assert!(updates.len() >= 3);

        assert_eq!(updates[0].status, JobExecutionStatus::InProgress as i32);
        assert_eq!(updates[0].std_out, "Test log line 1\n");
        assert_eq!(updates[1].status, JobExecutionStatus::InProgress as i32);
        assert_eq!(updates[1].std_out, "Test log line 2\n");
        assert_eq!(updates[2].status, JobExecutionStatus::Succeeded as i32);
        assert_eq!(updates[2].std_out, "");
    }
}
