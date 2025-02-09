use color_eyre::eyre::Result;
use orb_info::{OrbId, OrbJabilId, OrbName};
use orb_relay_client::{QoS, RecvMessage};
use orb_relay_messages::{
    orb_commands::v1::{OrbCommandError, OrbDetailsRequest, OrbDetailsResponse},
    prost::Message,
};
use tracing::info;

#[derive(Debug)]
pub struct OrbDetailsCommandHandler {
    orb_id: OrbId,
    orb_name: OrbName,
    jabil_id: OrbJabilId,
}

impl OrbDetailsCommandHandler {
    pub async fn new() -> Self {
        Self {
            orb_id: OrbId::new(),
            orb_name: OrbName::new(),
            jabil_id: OrbJabilId::new(),
        }
    }
}

impl OrbDetailsCommandHandler {
    #[tracing::instrument]
    pub async fn handle(&self, command: &RecvMessage) -> Result<(), OrbCommandError> {
        info!("Handling orb details command");
        let _request = OrbDetailsRequest::decode(command.payload.as_slice()).unwrap();
        // TODO(paulquinn00): Consult with @oldgalileo and @sfikastheo to determine where to get this info from.
        let response = OrbDetailsResponse {
            orb_id: self.orb_id.get().await.unwrap_or_default(),
            orb_name: self.orb_name.get().await.unwrap_or_default(),
            jabil_id: self.jabil_id.get().await.unwrap_or_default(),
            hardware_version: "".to_string(),
            software_version: "".to_string(),
            software_update_version: "".to_string(),
            os_release_type: "".to_string(),
            active_slot: "".to_string(),
            uptime_seconds: 0,
        };
        match command
            .reply(response.encode_to_vec(), QoS::AtLeastOnce)
            .await
        {
            Ok(_) => Ok(()),
            Err(_) => Err(OrbCommandError {
                error: "failed to send orb details response".to_string(),
            }),
        }
    }
}
