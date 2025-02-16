use color_eyre::eyre::Result;
use orb_relay_client::RecvMessage;
use tracing::info;

use super::JobActionError;

#[derive(Debug)]
pub struct OrbRebootActionHandler {}

impl OrbRebootActionHandler {
    pub fn new() -> Self {
        Self {}
    }
}

impl OrbRebootActionHandler {
    #[tracing::instrument]
    pub async fn handle(&self, _command: &RecvMessage) -> Result<(), JobActionError> {
        info!("Handling reboot command");
        Err(JobActionError::JobExecutionError(
            "Reboot command not implemented".to_string(),
        ))
    }
}
