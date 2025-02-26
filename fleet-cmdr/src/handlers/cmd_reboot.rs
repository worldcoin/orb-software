use color_eyre::eyre::{Error, Result};
use orb_relay_messages::fleet_cmdr::v1::{
    JobExecution, JobExecutionStatus, JobExecutionUpdate,
};
use tracing::info;

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
    ) -> Result<JobExecutionUpdate, Error> {
        info!("Handling reboot command");
        let response = JobExecutionUpdate {
            job_id: job.job_id.clone(),
            job_execution_id: job.job_execution_id.clone(),
            status: JobExecutionStatus::Failed as i32,
            std_out: "".to_string(),
            std_err: "not implemented".to_string(),
        };
        Ok(response)
    }
}
