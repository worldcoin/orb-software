use async_trait::async_trait;
use color_eyre::eyre::Result;
use orb_relay_client::RecvMessage;
use orb_relay_messages::orb_commands::v1::OrbCommandError;
use tracing::info;

use super::OrbCommandHandler;

pub struct OrbRebootCommandHandler {}

impl OrbRebootCommandHandler {
    pub fn new() -> Self {
        Self {}
    }
}

#[async_trait]
impl OrbCommandHandler for OrbRebootCommandHandler {
    async fn handle(&self, _command: &RecvMessage) -> Result<(), OrbCommandError> {
        info!("handling reboot command");
        Err(OrbCommandError {
            error: "reboot command not implemented".to_string(),
        })
    }
}
