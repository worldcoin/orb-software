mod cmd_orb_details;
mod cmd_reboot;

use cmd_orb_details::OrbDetailsCommand;
use cmd_reboot::RebootCommand;
use color_eyre::eyre::Result;
use orb_relay_messages::orb_commands::v1::{
    orb_command_issue, orb_command_result, OrbCommandError, OrbCommandIssue,
    OrbCommandResult, OrbDetailsRequest, RebootRequest,
};
use tracing::{info, warn};

pub struct OrbCommandHandlers {
    reboot: Box<dyn OrbCommandHandler<RebootRequest>>,
    orb_details: Box<dyn OrbCommandHandler<OrbDetailsRequest>>,
}

impl OrbCommandHandlers {
    pub fn new() -> Self {
        Self {
            reboot: Box::new(RebootCommand {}),
            orb_details: Box::new(OrbDetailsCommand {}),
        }
    }

    pub async fn handle_orb_command(
        &self,
        req: OrbCommandIssue,
    ) -> Result<OrbCommandResult, OrbCommandError> {
        match req.command {
            Some(command) => {
                info!("received command: {:?}", command);
                let result = match command {
                    orb_command_issue::Command::Reboot(req) => self.reboot.handle(req),
                    orb_command_issue::Command::OrbDetails(req) => {
                        self.orb_details.handle(req)
                    }
                };
                match result {
                    Ok(res) => Ok(OrbCommandResult {
                        command_id: req.command_id,
                        result: Some(res),
                    }),
                    Err(e) => Err(e),
                }
            }
            None => {
                warn!("received empty command");
                Err(OrbCommandError {
                    error: "empty command".to_string(),
                })
            }
        }
    }
}

pub trait OrbCommandHandler<T> {
    fn handle(&self, command: T)
        -> Result<orb_command_result::Result, OrbCommandError>;
}
