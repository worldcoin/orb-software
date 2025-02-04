mod cmd_orb_details;
mod cmd_reboot;

use color_eyre::eyre::Result;
use orb_relay_client::RecvMessage;
use orb_relay_messages::{
    orb_commands::v1::{OrbCommandError, OrbDetailsRequest, OrbRebootRequest},
    prost::{Message, Name},
    prost_types::Any,
};

pub struct OrbCommandHandlers {
    orb_details_handler: cmd_orb_details::OrbDetailsCommandHandler,
    reboot_handler: cmd_reboot::OrbRebootCommandHandler,
}

impl OrbCommandHandlers {
    pub fn init() -> Self {
        let orb_details_handler = cmd_orb_details::OrbDetailsCommandHandler::new();
        let reboot_handler = cmd_reboot::OrbRebootCommandHandler::new();
        Self {
            orb_details_handler,
            reboot_handler,
        }
    }

    pub async fn handle_orb_command(
        &self,
        msg: &RecvMessage,
    ) -> Result<(), OrbCommandError> {
        let any = Any::decode(msg.payload.as_slice()).unwrap();
        if any.type_url == OrbDetailsRequest::type_url() {
            self.orb_details_handler.handle(msg).await
        } else if any.type_url == OrbRebootRequest::type_url() {
            self.reboot_handler.handle(msg).await
        } else {
            Err(OrbCommandError {
                error: "No handler for command".to_string(),
            })
        }
    }
}
#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use orb_relay_client::{Amount, Auth, Client, ClientOpts, QoS, SendMessage};
    use orb_relay_messages::{
        orb_commands::v1::{OrbDetailsRequest, OrbDetailsResponse},
        prost::Message,
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
    async fn test_handle_orb_command() {
        // Arrange
        let sv = create_test_server().await;
        let client_svc =
            create_test_client("test_svc", "test_namespace", EntityType::Service, &sv)
                .await;
        let client_orb =
            create_test_client("test_orb", "test_namespace", EntityType::Orb, &sv)
                .await;
        let handlers = OrbCommandHandlers::init();

        // Act
        let request = OrbDetailsRequest {};
        let any = Any::from_msg(&request).unwrap();
        let msg = SendMessage::to(EntityType::Orb)
            .id("test_orb")
            .namespace("test_namespace")
            .qos(QoS::AtLeastOnce)
            .payload(any.encode_to_vec());

        // Assert
        task::spawn(async move {
            let msg = client_orb.recv().await.unwrap();
            let result = handlers.handle_orb_command(&msg).await;
            assert!(result.is_ok());
        });

        let result = client_svc.ask(msg).await;
        assert!(result.is_ok());
        let response = OrbDetailsResponse::decode(result.unwrap().as_slice());
        assert!(response.is_ok());
    }
}
