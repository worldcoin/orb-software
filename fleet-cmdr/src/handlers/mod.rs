mod action_orb_details;
mod action_reboot;

use color_eyre::eyre::Result;
use orb_relay_client::{QoS, RecvMessage};
use orb_relay_messages::{
    fleet_cmdr::v1::{JobExecution, JobNotify, JobRequestNext},
    prost::{Message, Name},
    prost_types::Any,
};
use tracing::{error, info};

#[derive(Debug, thiserror::Error)]
pub enum JobActionError {
    #[error("job action in progress")]
    JobActionInProgress,
    #[error("no handler for command")]
    NoHandlerForCommand,
    #[error("job execution error: {0}")]
    JobExecutionError(String),
}

pub struct JobActionHandlers {
    orb_details_handler: action_orb_details::OrbDetailsActionHandler,
    reboot_handler: action_reboot::OrbRebootActionHandler,
}

impl JobActionHandlers {
    pub async fn init() -> Self {
        let orb_details_handler =
            action_orb_details::OrbDetailsActionHandler::new().await;
        let reboot_handler = action_reboot::OrbRebootActionHandler::new();
        Self {
            orb_details_handler,
            reboot_handler,
        }
    }

    pub async fn handle_msg(&self, msg: &RecvMessage) -> Result<(), JobActionError> {
        let any = Any::decode(msg.payload.as_slice()).map_err(|_| {
            JobActionError::JobExecutionError("failed to decode any".to_string())
        })?;
        if any.type_url == JobNotify::type_url() {
            info!("Received job notify");
            self.request_next(msg).await
        } else if any.type_url == JobExecution::type_url() {
            info!("Received job execution");
            match self.handle_job_execution(&any, msg).await {
                Ok(_) => self.request_next(msg).await,
                Err(JobActionError::JobActionInProgress) => {
                    info!("Job action in progress, skipping request next job");
                    Ok(())
                }
                Err(e) => {
                    error!("Error handling job execution: {:?}", e);
                    Err(e)
                }
            }
        } else {
            error!("Unknown message type: {:?}", any.type_url);
            Err(JobActionError::NoHandlerForCommand)
        }
    }

    async fn handle_job_execution(
        &self,
        any: &Any,
        msg: &RecvMessage,
    ) -> Result<(), JobActionError> {
        let job = JobExecution::decode(any.value.as_slice()).unwrap();
        info!("Handling job execution: {:?}", job);
        match job.command.as_str() {
            "orb_details" => self.orb_details_handler.handle(msg).await,
            "reboot" => self.reboot_handler.handle(msg).await,
            _ => Err(JobActionError::NoHandlerForCommand),
        }
    }

    async fn request_next(&self, msg: &RecvMessage) -> Result<(), JobActionError> {
        let response = Any::from_msg(&JobRequestNext::default()).unwrap();
        msg.reply(response.encode_to_vec(), QoS::AtLeastOnce)
            .await
            .map_err(|_| {
                JobActionError::JobExecutionError(
                    "failed to request next job".to_string(),
                )
            })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use orb_relay_client::{Amount, Auth, Client, ClientOpts, QoS, SendMessage};
    use orb_relay_messages::{
        fleet_cmdr::v1::JobExecutionUpdate,
        prost_types::Any,
        relay::{
            entity::EntityType, relay_connect_request::Msg, ConnectRequest,
            ConnectResponse,
        },
    };
    use orb_relay_test_utils::{IntoRes, TestServer};
    use tokio::{self, task};

    struct NoState;

    async fn create_test_server() -> TestServer<NoState> {
        TestServer::new(NoState, move |_state, conn_req, clients| match conn_req {
            Msg::ConnectRequest(ConnectRequest { client_id, .. }) => ConnectResponse {
                client_id: client_id.unwrap().id.clone(),
                success: true,
                error: "Nothing".to_string(),
            }
            .into_res(),

            Msg::Payload(payload) => {
                clients.send(payload);
                None
            }

            _ => None,
        })
        .await
    }

    async fn create_test_client(
        id: &str,
        namespace: &str,
        entity_type: EntityType,
        test_server: &TestServer<NoState>,
    ) -> Client {
        let opts = ClientOpts::entity(entity_type)
            .id(id)
            .namespace(namespace)
            .endpoint(format!("http://{}", test_server.addr()))
            .auth(Auth::Token(Default::default()))
            .max_connection_attempts(Amount::Val(1))
            .connection_timeout(Duration::from_millis(10))
            .heartbeat(Duration::from_secs(u64::MAX))
            .ack_timeout(Duration::from_millis(10))
            .build();

        let (client, _handle) = Client::connect(opts);
        client
    }
    #[tokio::test]
    async fn test_handle_job_notify() {
        // Arrange
        let sv = create_test_server().await;
        let client_svc =
            create_test_client("test_svc", "test_namespace", EntityType::Service, &sv)
                .await;
        let client_orb =
            create_test_client("test_orb", "test_namespace", EntityType::Orb, &sv)
                .await;
        let handlers = JobActionHandlers::init().await;

        // Act
        let request = JobNotify {};
        let any = Any::from_msg(&request).unwrap();
        let msg = SendMessage::to(EntityType::Orb)
            .id("test_orb")
            .namespace("test_namespace")
            .qos(QoS::AtLeastOnce)
            .payload(any.encode_to_vec());

        // Assert
        task::spawn(async move {
            let msg = client_orb.recv().await.unwrap();
            let result = handlers.handle_msg(&msg).await;
            assert!(result.is_ok());
        });

        let result = client_svc.ask(msg).await;
        assert!(result.is_ok());
        let response = JobRequestNext::decode(result.unwrap().as_slice()).unwrap();
        assert_eq!(response, JobRequestNext {});
    }

    #[tokio::test]
    async fn test_handle_job_execution() {
        // Arrange
        let sv = create_test_server().await;
        let client_svc =
            create_test_client("test_svc", "test_namespace", EntityType::Service, &sv)
                .await;
        let client_orb =
            create_test_client("test_orb", "test_namespace", EntityType::Orb, &sv)
                .await;
        let handlers = JobActionHandlers::init().await;

        // Act
        let request = JobExecution {
            command: "orb_details".to_string(),
            ..Default::default()
        };
        let any = Any::from_msg(&request).unwrap();
        let msg = SendMessage::to(EntityType::Orb)
            .id("test_orb")
            .namespace("test_namespace")
            .qos(QoS::AtLeastOnce)
            .payload(any.encode_to_vec());

        // Assert
        task::spawn(async move {
            let msg = client_orb.recv().await.unwrap();
            let result = handlers.handle_msg(&msg).await;
            assert!(result.is_ok());
        });

        let result = client_svc.ask(msg).await;
        assert!(result.is_ok());
        let response = JobExecutionUpdate::decode(result.unwrap().as_slice());
        assert!(response.is_ok());
    }
}
