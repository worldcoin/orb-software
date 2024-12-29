use color_eyre::eyre::Result;
use orb_relay_messages::orb_commands::v1::{
    orb_command_result, OrbCommandError, RebootRequest,
};
use tracing::info;

use super::OrbCommandHandler;

pub struct RebootCommand {}

impl OrbCommandHandler<RebootRequest> for RebootCommand {
    fn handle(
        &self,
        _command: RebootRequest,
    ) -> Result<orb_command_result::Result, OrbCommandError> {
        info!("handling reboot command");
        Err(OrbCommandError {
            error: "reboot command not implemented".to_string(),
        })
    }
}
