use color_eyre::eyre::Result;
use orb_info::{OrbJabilId, OrbName};
use orb_relay_client::{QoS, RecvMessage};
use orb_relay_messages::{
    fleet_cmdr::v1::{JobExecution, JobExecutionStatus, JobExecutionUpdate},
    prost::Message,
    prost_types::Any,
};
use serde::Serialize;
use tracing::info;

use super::JobActionError;

#[derive(Debug, Serialize)]
pub struct OrbDetailsActionHandler {
    orb_name: String,
    jabil_id: String,
}

impl OrbDetailsActionHandler {
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

impl OrbDetailsActionHandler {
    #[tracing::instrument]
    pub async fn handle(
        &self,
        msg: &RecvMessage,
        job: &JobExecution,
    ) -> Result<(), JobActionError> {
        info!("Handling orb details command");
        let response = JobExecutionUpdate {
            job_id: job.job_id.clone(),
            job_execution_id: job.job_execution_id.clone(),
            status: JobExecutionStatus::Completed as i32,
            std_out: serde_json::to_string(&self).unwrap(),
            std_err: "".to_string(),
        };
        let any = Any::from_msg(&response).unwrap();
        match msg.reply(any.encode_to_vec(), QoS::AtLeastOnce).await {
            Ok(_) => Ok(()),
            Err(_) => Err(JobActionError::JobExecutionError(
                "failed to send orb details response".to_string(),
            )),
        }
    }
}
