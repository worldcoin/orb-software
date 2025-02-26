use color_eyre::eyre::{Error, Result};
use orb_info::{OrbJabilId, OrbName};
use orb_relay_messages::fleet_cmdr::v1::{
    JobExecution, JobExecutionStatus, JobExecutionUpdate,
};
use tracing::info;

#[derive(Debug)]
pub struct OrbDetailsCommandHandler {
    orb_name: String,
    jabil_id: String,
}

impl OrbDetailsCommandHandler {
    pub async fn new() -> Self {
        Self {
            orb_name: OrbName::read()
                .await
                .map_or("NO_ORB_NAME".to_string(), |orb_name| orb_name.to_string()),
            jabil_id: OrbJabilId::read()
                .await
                .map_or("NO_JABIL_ID".to_string(), |jabil_id| jabil_id.to_string()),
        }
    }
}

impl OrbDetailsCommandHandler {
    #[tracing::instrument]
    pub async fn handle(
        &self,
        command: &JobExecution,
    ) -> Result<JobExecutionUpdate, Error> {
        info!("Handling orb details command");
        let response = JobExecutionUpdate {
            job_id: command.job_id.clone(),
            job_execution_id: command.job_execution_id.clone(),
            status: JobExecutionStatus::Completed as i32,
            std_out: serde_json::json!({
                "orb_name": self.orb_name,
                "jabil_id": self.jabil_id,
            })
            .to_string(),
            std_err: "".to_string(),
        };
        Ok(response)
    }
}
