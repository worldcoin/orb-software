use color_eyre::eyre::{Context, Error, Result};
use orb_relay_messages::fleet_cmdr::v1::{
    JobExecution, JobExecutionStatus, JobExecutionUpdate,
};
use std::{
    io::{Read, Write},
    path::Path,
};
use tracing::{error, info};

use crate::job_client::JobClient;

#[derive(Debug)]
pub struct OrbRebootCommandHandler {}

impl OrbRebootCommandHandler {
    pub fn new() -> Self {
        Self {}
    }
}

impl OrbRebootCommandHandler {
    #[tracing::instrument]
    pub async fn handle(
        &self,
        job: &JobExecution,
        job_client: &JobClient,
    ) -> Result<JobExecutionUpdate, Error> {
        info!("Handling reboot command");
        // If the lock file doesn't exist or the job execution id doesn't match, issue a reboot.
        // This is to avoid being stuck behind the lock file if a job fails to reboot the orb
        let job_execution_id = Self::read_reboot_lock_file(job)?;
        info!("existing job_execution_id: {:?}", job_execution_id);
        let do_reboot = job_execution_id.is_none()
            || job_execution_id.clone().unwrap() != job.job_execution_id;
        if do_reboot {
            info!(
                "Rebooting orb due to job execution {}",
                job.job_execution_id
            );
            Self::remove_reboot_lock_file(job)?;
            Self::create_reboot_lock_file(job)?;
            #[cfg(target_os = "linux")]
            Self::reboot_orb().await?;
            #[cfg(target_os = "macos")]
            Self::reboot_orb(job_client).await?;
            Ok(JobExecutionUpdate {
                job_id: job.job_id.clone(),
                job_execution_id: job.job_execution_id.clone(),
                status: JobExecutionStatus::InProgress as i32,
                std_out: "rebooting".to_string(),
                std_err: "".to_string(),
            })
        } else {
            info!("Orb rebooted due to job execution {:?}", job_execution_id);
            Self::remove_reboot_lock_file(job)?;
            Ok(JobExecutionUpdate {
                job_id: job.job_id.clone(),
                job_execution_id: job.job_execution_id.clone(),
                status: JobExecutionStatus::Succeeded as i32,
                std_out: "rebooted".to_string(),
                std_err: "".to_string(),
            })
        }
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
    async fn reboot_orb(job_client: &JobClient) -> Result<()> {
        // Use this delay to fake a reboot on macos
        let job_client_clone = job_client.clone();
        let _handle = tokio::task::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            let _ = job_client_clone.request_next_job().await;
        });
        Ok(())
    }
}

#[cfg(test)]
#[cfg(target_os = "macos")]
mod tests {
    use super::*;
    use crate::handlers::tests::{create_test_client, create_test_server};
    use orb_relay_messages::{
        fleet_cmdr::v1::{JobExecution, JobExecutionStatus},
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
        };

        let test_server = create_test_server().await;
        let client = create_test_client(
            "test_client",
            "test_namespace",
            EntityType::Orb,
            &test_server,
        )
        .await;
        let job_client = JobClient::new(client, "test_client", "test_namespace");

        // Remove the lock file if it exists
        let lock_file = format!("/tmp/reboot_{}.lock", job.job_execution_id);
        std::fs::remove_file(&lock_file).unwrap_or_default();

        // Execute
        let result = handler.handle(&job, &job_client).await.unwrap();

        // Verify
        assert_eq!(result.job_id, "job123");
        assert_eq!(result.job_execution_id, "exec456");
        assert_eq!(result.status, JobExecutionStatus::InProgress as i32);
        assert_eq!(result.std_out, "rebooting");
        assert_eq!(result.std_err, "");

        // Verify the lock file was created
        assert!(Path::new(&lock_file).exists());

        // Verify the content of the lock file
        let content = std::fs::read_to_string(&lock_file).unwrap();
        assert_eq!(content, "exec456");
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
        };

        let test_server = create_test_server().await;
        let client = create_test_client(
            "test_client",
            "test_namespace",
            EntityType::Orb,
            &test_server,
        )
        .await;
        let job_client = JobClient::new(client, "test_client", "test_namespace");

        // Create a lock file to simulate a reboot
        let lock_file = format!("/tmp/reboot_{}.lock", job.job_execution_id);
        std::fs::write(&lock_file, "exec456").unwrap();

        // Execute
        let result = handler.handle(&job, &job_client).await.unwrap();

        // Verify
        assert_eq!(result.job_id, "job123");
        assert_eq!(result.job_execution_id, "exec456");
        assert_eq!(result.status, JobExecutionStatus::Succeeded as i32);
        assert_eq!(result.std_out, "rebooted");
        assert_eq!(result.std_err, "");

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
        };

        let test_server = create_test_server().await;
        let client = create_test_client(
            "test_client",
            "test_namespace",
            EntityType::Orb,
            &test_server,
        )
        .await;
        let job_client = JobClient::new(client, "test_client", "test_namespace");
        // Create a lock file with a different job execution id
        let lock_file = format!("/tmp/reboot_{}.lock", job.job_execution_id);
        std::fs::write(&lock_file, "different_exec_id").unwrap();

        // Execute
        let result = handler.handle(&job, &job_client).await.unwrap();

        // Verify that we got an InProgress status (indicating a reboot)
        assert_eq!(result.job_id, "job123");
        assert_eq!(result.job_execution_id, "exec456");
        assert_eq!(result.status, JobExecutionStatus::InProgress as i32);
        assert_eq!(result.std_out, "rebooting");
        assert_eq!(result.std_err, "");

        // Verify the lock file was updated with the new job execution id
        let content = std::fs::read_to_string(&lock_file).unwrap();
        assert_eq!(content, "exec456");
    }
}
