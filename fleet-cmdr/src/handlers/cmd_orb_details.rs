use color_eyre::eyre::Result;
use orb_relay_messages::orb_commands::v1::{
    orb_command_result, OrbCommandError, OrbDetailsRequest, OrbDetailsResponse,
};
use tracing::info;

use super::OrbCommandHandler;

pub struct OrbDetailsCommand {}

impl OrbCommandHandler<OrbDetailsRequest> for OrbDetailsCommand {
    fn handle(
        &self,
        _command: OrbDetailsRequest,
    ) -> Result<orb_command_result::Result, OrbCommandError> {
        info!("handling orb details command");
        Ok(orb_command_result::Result::OrbDetails(OrbDetailsResponse {
            orb_id: "".to_string(),
            orb_name: "".to_string(),
            jabil_id: "".to_string(),
            hardware_version: "".to_string(),
            software_version: "".to_string(),
            software_update_version: "".to_string(),
            os_release_type: "".to_string(),
            active_slot: "".to_string(),
            uptime_seconds: 0,
        }))
    }
}
