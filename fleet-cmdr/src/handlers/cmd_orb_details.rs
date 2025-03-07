use color_eyre::eyre::{Error, Result};
use orb_info::{OrbJabilId, OrbName};
use orb_relay_messages::fleet_cmdr::v1::{
    JobExecution, JobExecutionStatus, JobExecutionUpdate,
};
use tracing::info;

#[derive(Debug)]
pub struct OrbDetailsCommandHandler {}

impl OrbDetailsCommandHandler {
    pub fn new() -> Self {
        Self {}
    }
}

impl OrbDetailsCommandHandler {
    #[tracing::instrument]
    pub async fn handle(
        &self,
        command: &JobExecution,
    ) -> Result<JobExecutionUpdate, Error> {
        info!("Handling orb details command");
        Ok(JobExecutionUpdate {
            job_id: command.job_id.clone(),
            job_execution_id: command.job_execution_id.clone(),
            status: JobExecutionStatus::Succeeded as i32,
            std_out: serde_json::json!({
                "orb_name": OrbName::read().await.unwrap_or(OrbName("NO_ORB_NAME".to_string())).to_string(),
                "jabil_id": OrbJabilId::read().await.unwrap_or(OrbJabilId("NO_JABIL_ID".to_string())).to_string(),
            })
            .to_string(),
            std_err: "".to_string(),
        })
    }
}
