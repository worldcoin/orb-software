use color_eyre::eyre::{Context, Error, Result};
use orb_relay_client::Client;
use orb_relay_messages::fleet_cmdr::v1::{
    JobExecution, JobExecutionStatus, JobExecutionUpdate,
};
use std::{
    io::{Read, Write},
    path::Path,
};
use tracing::{error, info};

#[cfg(target_os = "macos")]
use crate::handlers::send_job_request;
#[cfg(target_os = "linux")]
use orb_zbus_proxies::login1;
#[cfg(target_os = "linux")]
use tracing::debug;

const REBOOT_LOCK_FILE: &str = "/tmp/reboot.lock";
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
        relay_client: &Client,
    ) -> Result<JobExecutionUpdate, Error> {
        info!("Handling reboot command");
        // If the lock file doesn't exist or the job execution id doesn't match, issue a reboot.
        // This is to avoid being stuck behind the lock file if a job fails to reboot the orb
        let job_execution_id = Self::read_reboot_lock_file()?;
        let do_reboot =
            job_execution_id.is_empty() || job_execution_id != job.job_execution_id;
        if do_reboot {
            info!(
                "Rebooting orb due to job execution {}",
                job.job_execution_id
            );
            Self::remove_reboot_lock_file()?;
            Self::create_reboot_lock_file(job)?;
            #[cfg(target_os = "linux")]
            Self::reboot_orb()?;
            #[cfg(target_os = "macos")]
            Self::reboot_orb(job, relay_client).await?;
            Ok(JobExecutionUpdate {
                job_id: job.job_id.clone(),
                job_execution_id: job.job_execution_id.clone(),
                status: JobExecutionStatus::InProgress as i32,
                std_out: "rebooting".to_string(),
                std_err: "".to_string(),
            })
        } else {
            info!("Orb rebooted due to job execution {}", job_execution_id);
            Self::remove_reboot_lock_file()?;
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
        std::fs::create_dir_all(Path::new(REBOOT_LOCK_FILE).parent().unwrap())?;
        let mut file = std::fs::File::create(Path::new(REBOOT_LOCK_FILE))
            .wrap_err_with(|| {
                format!("Failed to create reboot file at {:?}", REBOOT_LOCK_FILE)
            })?;
        file.write_all(job.job_execution_id.as_bytes())
            .map_err(|e| {
                error!("Failed to write job execution id to reboot file: {}", e);
                Error::new(e)
            })?;
        Ok(())
    }

    fn read_reboot_lock_file() -> Result<String> {
        let mut file = match std::fs::File::open(REBOOT_LOCK_FILE) {
            Ok(file) => file,
            Err(e) => {
                if e.kind() == std::io::ErrorKind::NotFound {
                    return Ok(String::new());
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
        Ok(contents)
    }

    fn remove_reboot_lock_file() -> Result<()> {
        match std::fs::remove_file(REBOOT_LOCK_FILE) {
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
    fn reboot_orb() -> Result<()> {
        zbus::blocking::Connection::system()
            .wrap_err("failed establishing a `systemd` dbus connection")
            .and_then(|conn| {
                login1::ManagerProxyBlocking::new(&conn)
                    .wrap_err("failed creating systemd1 Manager proxy")
            })
            .and_then(|proxy| {
                debug!(
                    "scheduling poweroff in 0ms by calling \
                 org.freedesktop.login1.Manager.ScheduleShutdown"
                );
                proxy.schedule_shutdown("reboot", 0).wrap_err(
                    "failed issuing scheduled reboot to \
                 org.freedesktop.login1.Manager.ScheduleShutdown",
                )
            })
    }

    #[cfg(target_os = "macos")]
    async fn reboot_orb(job: &JobExecution, relay_client: &Client) -> Result<()> {
        // Use this delay to fake a reboot on macos
        let job_clone = job.clone();
        let relay_client_clone = relay_client.clone();
        let _handle = tokio::task::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            let _ = send_job_request(
                &relay_client_clone,
                job_clone.job_id.as_str(),
                job_clone.job_execution_id.as_str(),
            )
            .await;
        });
        Ok(())
    }
}

#[cfg(test)]
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

        // Remove the lock file if it exists
        std::fs::remove_file(REBOOT_LOCK_FILE).unwrap_or_default();

        // Execute
        let result = handler.handle(&job, &client).await.unwrap();

        // Verify
        assert_eq!(result.job_id, "job123");
        assert_eq!(result.job_execution_id, "exec456");
        assert_eq!(result.status, JobExecutionStatus::InProgress as i32);
        assert_eq!(result.std_out, "rebooting");
        assert_eq!(result.std_err, "");

        // Verify the lock file was created
        assert!(Path::new(REBOOT_LOCK_FILE).exists());

        // Verify the content of the lock file
        let content = std::fs::read_to_string(REBOOT_LOCK_FILE).unwrap();
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

        // Create a lock file to simulate a reboot
        std::fs::write(REBOOT_LOCK_FILE, "exec456").unwrap();

        // Execute
        let result = handler.handle(&job, &client).await.unwrap();

        // Verify
        assert_eq!(result.job_id, "job123");
        assert_eq!(result.job_execution_id, "exec456");
        assert_eq!(result.status, JobExecutionStatus::Succeeded as i32);
        assert_eq!(result.std_out, "rebooted");
        assert_eq!(result.std_err, "");

        // Verify the lock file was removed
        assert!(!Path::new(REBOOT_LOCK_FILE).exists());
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

        // Create a lock file with a different job execution id
        std::fs::write(REBOOT_LOCK_FILE, "different_exec_id").unwrap();

        // Execute
        let result = handler.handle(&job, &client).await.unwrap();

        // Verify that we got an InProgress status (indicating a reboot)
        assert_eq!(result.job_id, "job123");
        assert_eq!(result.job_execution_id, "exec456");
        assert_eq!(result.status, JobExecutionStatus::InProgress as i32);
        assert_eq!(result.std_out, "rebooting");
        assert_eq!(result.std_err, "");

        // Verify the lock file was updated with the new job execution id
        let content = std::fs::read_to_string(REBOOT_LOCK_FILE).unwrap();
        assert_eq!(content, "exec456");
    }
}
