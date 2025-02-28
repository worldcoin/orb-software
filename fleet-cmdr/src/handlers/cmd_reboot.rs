use color_eyre::eyre::{Error, Result};
use orb_relay_client::Client;
use orb_relay_messages::fleet_cmdr::v1::{
    JobExecution, JobExecutionStatus, JobExecutionUpdate,
};
use tracing::{error, info};

use crate::handlers::send_job_request;

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
        let path =
            std::env::temp_dir().join(format!("reboot.{}", job.job_execution_id));
        let response = if !path.exists() {
            // I'm going to reboot.
            info!(
                "Rebooting orb due to job execution {}",
                job.job_execution_id
            );
            std::fs::File::create(path).map_err(|e| {
                error!("Failed to create reboot file: {}", e);
                Error::new(e)
            })?;
            // TODO: Send dbus message to the orb to reboot.
            // Use this fake delay for now.
            let job_clone = job.clone();
            let relay_client_clone = relay_client.clone();
            tokio::task::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(4)).await;
                let _ = send_job_request(
                    &relay_client_clone,
                    job_clone.job_id.as_str(),
                    job_clone.job_execution_id.as_str(),
                )
                .await;
            });
            JobExecutionUpdate {
                job_id: job.job_id.clone(),
                job_execution_id: job.job_execution_id.clone(),
                status: JobExecutionStatus::Running as i32,
                std_out: "rebooting".to_string(),
                std_err: "".to_string(),
            }
        } else {
            // I'm back from a reboot.
            info!("Orb rebooted due to job execution {}", job.job_execution_id);
            std::fs::remove_file(path).map_err(|e| {
                error!("Failed to remove reboot file: {}", e);
                Error::new(e)
            })?;
            JobExecutionUpdate {
                job_id: job.job_id.clone(),
                job_execution_id: job.job_execution_id.clone(),
                status: JobExecutionStatus::Completed as i32,
                std_out: "rebooted".to_string(),
                std_err: "".to_string(),
            }
        };
        Ok(response)
    }
}
