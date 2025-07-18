use color_eyre::eyre::{Context, Error, Result};
use orb_relay_messages::jobs::v1::{
    JobExecution, JobExecutionStatus, JobExecutionUpdate,
};
use std::{
    io::{Read, Write},
    path::Path,
};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use crate::job_client::JobClient;
use crate::orchestrator::JobCompletion;

#[derive(Debug, Clone)]
pub struct OrbRebootCommandHandler {}

impl OrbRebootCommandHandler {
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
        info!("Handling reboot command for job {}", job.job_execution_id);

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

        // Check if this is a reboot completion check or a new reboot request
        let job_execution_id = Self::read_reboot_lock_file(job)?;
        info!("existing job_execution_id: {:?}", job_execution_id);
        let do_reboot = job_execution_id.is_none()
            || job_execution_id.clone().unwrap() != job.job_execution_id;

        if do_reboot {
            // This is a new reboot request
            info!(
                "Rebooting orb due to job execution {}",
                job.job_execution_id
            );
            Self::remove_reboot_lock_file(job)?;
            Self::create_reboot_lock_file(job)?;

            // Send initial progress update
            let update = JobExecutionUpdate {
                job_id: job.job_id.clone(),
                job_execution_id: job.job_execution_id.clone(),
                status: JobExecutionStatus::InProgress as i32,
                std_out: "rebooting".to_string(),
                std_err: String::new(),
            };

            if let Err(e) = job_client.send_job_update(&update).await {
                error!("Failed to send job update: {:?}", e);
            }

            // Initiate reboot
            #[cfg(target_os = "linux")]
            {
                Self::reboot_orb().await?;
                // On Linux, the system will actually reboot, so we won't reach completion
                // The job will be re-processed after reboot
            }

            #[cfg(target_os = "macos")]
            {
                Self::reboot_orb_macos(job_client, job, completion_tx, cancel_token)
                    .await?;
            }
        } else {
            // This is a reboot completion check
            info!("Orb rebooted due to job execution {:?}", job_execution_id);
            Self::remove_reboot_lock_file(job)?;

            let update = JobExecutionUpdate {
                job_id: job.job_id.clone(),
                job_execution_id: job.job_execution_id.clone(),
                status: JobExecutionStatus::Succeeded as i32,
                std_out: "rebooted".to_string(),
                std_err: String::new(),
            };

            if let Err(e) = job_client.send_job_update(&update).await {
                error!("Failed to send job update: {:?}", e);
            }
            completion_tx
                .send(JobCompletion::success(job.job_execution_id.clone()))
                .ok();
        }

        Ok(())
    }

    fn create_reboot_lock_file(job: &JobExecution) -> Result<()> {
        let lock_file = format!("/tmp/reboot_{}.lock", job.job_execution_id);
        std::fs::create_dir_all(Path::new(&lock_file).parent().unwrap())?;
        let mut file =
            std::fs::File::create(Path::new(&lock_file)).wrap_err_with(|| {
                format!("Failed to create reboot file at {:?}", lock_file)
            })?;
        file.write_all(job.job_execution_id.as_bytes())
            .map_err(|e| {
                error!("Failed to write job execution id to reboot file: {}", e);
                Error::new(e)
            })?;
        Ok(())
    }

    fn read_reboot_lock_file(job: &JobExecution) -> Result<Option<String>> {
        let lock_file = format!("/tmp/reboot_{}.lock", job.job_execution_id);
        let mut file = match std::fs::File::open(lock_file) {
            Ok(file) => file,
            Err(e) => {
                if e.kind() == std::io::ErrorKind::NotFound {
                    return Ok(None);
                } else {
                    error!("Failed to open reboot file: {}", e);
                    return Err(e.into());
                }
            }
        };
        let mut contents = String::new();
        file.read_to_string(&mut contents).map_err(|e| {
            error!("Failed to read job execution id from reboot file: {}", e);
            Error::new(e)
        })?;
        Ok(Some(contents))
    }

    fn remove_reboot_lock_file(job: &JobExecution) -> Result<()> {
        let lock_file = format!("/tmp/reboot_{}.lock", job.job_execution_id);
        match std::fs::remove_file(lock_file) {
            Ok(_) => Ok(()),
            Err(e) => {
                if e.kind() == std::io::ErrorKind::NotFound {
                    Ok(())
                } else {
                    error!("Failed to remove reboot file: {}", e);
                    Err(e.into())
                }
            }
        }
    }

    #[cfg(target_os = "linux")]
    async fn reboot_orb() -> Result<()> {
        let conn = zbus::Connection::system()
            .await
            .wrap_err("failed establishing a `systemd` dbus connection")?;
        let proxy = orb_zbus_proxies::login1::ManagerProxy::new(&conn).await?;
        proxy.schedule_shutdown("reboot", 5).await?;
        Ok(())
    }

    #[cfg(target_os = "macos")]
    async fn reboot_orb_macos(
        job_client: &JobClient,
        job: &JobExecution,
        completion_tx: oneshot::Sender<JobCompletion>,
        cancel_token: CancellationToken,
    ) -> Result<()> {
        // Use this delay to fake a reboot on macOS
        let job_client_clone = job_client.clone();
        let job_clone = job.clone();

        tokio::spawn(async move {
            // Simulate reboot delay
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;

            // Check if job was cancelled during reboot
            if cancel_token.is_cancelled() {
                let update = JobExecutionUpdate {
                    job_id: job_clone.job_id.clone(),
                    job_execution_id: job_clone.job_execution_id.clone(),
                    status: JobExecutionStatus::Failed as i32,
                    std_out: String::new(),
                    std_err: "Reboot cancelled".to_string(),
                };

                let _ = job_client_clone.send_job_update(&update).await;
                completion_tx
                    .send(JobCompletion::cancelled(job_clone.job_execution_id.clone()))
                    .ok();
                return;
            }

            // Clean up the lock file (simulate reboot completion)
            let _ = Self::remove_reboot_lock_file(&job_clone);

            // Send final job update indicating reboot completed
            let job_update = JobExecutionUpdate {
                job_id: job_clone.job_id.clone(),
                job_execution_id: job_clone.job_execution_id.clone(),
                status: JobExecutionStatus::Succeeded as i32,
                std_out: "rebooted".to_string(),
                std_err: String::new(),
            };

            let _ = job_client_clone.send_job_update(&job_update).await;
            completion_tx
                .send(JobCompletion::success(job_clone.job_execution_id.clone()))
                .ok();
        });

        Ok(())
    }
}

#[cfg(test)]
#[cfg(target_os = "macos")]
mod tests {
    use super::*;
    use crate::handlers::tests::{create_test_client, create_test_server};
    use crate::orchestrator::JobRegistry;
    use orb_relay_messages::{
        jobs::v1::{JobExecution, JobExecutionStatus},
        relay::entity::EntityType,
    };

    #[tokio::test]
    #[serial_test::serial]
    async fn test_handle_reboot_command_first_call() {
        // Setup
        let handler = OrbRebootCommandHandler::new();
        let job = JobExecution {
            job_id: "job123".to_string(),
            job_execution_id: "exec456".to_string(),
            job_document: "reboot".to_string(),
            should_cancel: false,
        };

        let test_server = create_test_server().await;
        let client = create_test_client(
            "test_client",
            "test_namespace",
            EntityType::Orb,
            &test_server,
        )
        .await;
        let job_client = JobClient::new(
            client,
            "test_client",
            "test_namespace",
            JobRegistry::new(),
            crate::orchestrator::JobConfig::new(),
        );

        // Remove the lock file if it exists
        let lock_file = format!("/tmp/reboot_{}.lock", job.job_execution_id);
        std::fs::remove_file(&lock_file).unwrap_or_default();

        let (completion_tx, completion_rx) = oneshot::channel();
        let cancel_token = CancellationToken::new();

        // Execute
        let result = handler
            .handle(&job, &job_client, completion_tx, cancel_token)
            .await;
        assert!(result.is_ok());

        // Wait for completion
        let completion = completion_rx.await.unwrap();
        assert_eq!(completion.status, JobExecutionStatus::Succeeded);
        assert_eq!(completion.job_execution_id, "exec456");

        // Verify the lock file was removed after completion
        assert!(!Path::new(&lock_file).exists());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_handle_reboot_command_after_reboot() {
        // Setup
        let handler = OrbRebootCommandHandler::new();
        let job = JobExecution {
            job_id: "job123".to_string(),
            job_execution_id: "exec456".to_string(),
            job_document: "reboot".to_string(),
            should_cancel: false,
        };

        let test_server = create_test_server().await;
        let client = create_test_client(
            "test_client",
            "test_namespace",
            EntityType::Orb,
            &test_server,
        )
        .await;
        let job_client = JobClient::new(
            client,
            "test_client",
            "test_namespace",
            JobRegistry::new(),
            crate::orchestrator::JobConfig::new(),
        );

        // Create a lock file to simulate a reboot
        let lock_file = format!("/tmp/reboot_{}.lock", job.job_execution_id);
        std::fs::write(&lock_file, "exec456").unwrap();

        let (completion_tx, completion_rx) = oneshot::channel();
        let cancel_token = CancellationToken::new();

        // Execute
        let result = handler
            .handle(&job, &job_client, completion_tx, cancel_token)
            .await;
        assert!(result.is_ok());

        // Wait for completion
        let completion = completion_rx.await.unwrap();
        assert_eq!(completion.status, JobExecutionStatus::Succeeded);
        assert_eq!(completion.job_execution_id, "exec456");

        // Verify the lock file was removed
        assert!(!Path::new(&lock_file).exists());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_handle_reboot_command_job_id_mismatch() {
        // Setup
        let handler = OrbRebootCommandHandler::new();
        let job = JobExecution {
            job_id: "job123".to_string(),
            job_execution_id: "exec456".to_string(),
            job_document: "reboot".to_string(),
            should_cancel: false,
        };

        let test_server = create_test_server().await;
        let client = create_test_client(
            "test_client",
            "test_namespace",
            EntityType::Orb,
            &test_server,
        )
        .await;
        let job_client = JobClient::new(
            client,
            "test_client",
            "test_namespace",
            JobRegistry::new(),
            crate::orchestrator::JobConfig::new(),
        );

        // Create a lock file with a different job execution id
        let lock_file = format!("/tmp/reboot_{}.lock", job.job_execution_id);
        std::fs::write(&lock_file, "different_exec_id").unwrap();

        let (completion_tx, completion_rx) = oneshot::channel();
        let cancel_token = CancellationToken::new();

        // Execute
        let result = handler
            .handle(&job, &job_client, completion_tx, cancel_token)
            .await;
        assert!(result.is_ok());

        // Wait for completion
        let completion = completion_rx.await.unwrap();
        assert_eq!(completion.status, JobExecutionStatus::Succeeded);
        assert_eq!(completion.job_execution_id, "exec456");

        // Verify the lock file was removed
        assert!(!Path::new(&lock_file).exists());
    }
}
