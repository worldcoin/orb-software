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
            orb_id: OrbId::read().await.unwrap(),
            orb_name: OrbName::read().await.unwrap(),
            jabil_id: OrbJabilId::read().await.unwrap(),
        }
    }
}

impl OrbDetailsCommandHandler {
    #[tracing::instrument]
    pub async fn handle(&self, command: &RecvMessage) -> Result<(), OrbCommandError> {
        info!("Handling orb details command");
        let _request = OrbDetailsRequest::decode(command.payload.as_slice()).unwrap();
        let response = OrbDetailsResponse {
            orb_id: self.orb_id.to_string(),
            orb_name: self.orb_name.value().to_string(),
            jabil_id: self.jabil_id.value().to_string(),
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
