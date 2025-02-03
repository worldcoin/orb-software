mod cmd_orb_details;
mod cmd_reboot;

use async_trait::async_trait;
use color_eyre::eyre::Result;
use orb_relay_client::RecvMessage;
use orb_relay_messages::{
    orb_commands::v1::{OrbCommandError, OrbDetailsRequest, OrbRebootRequest},
    prost::{Message, Name},
    prost_types::Any,
};
use std::collections::HashMap;
use tracing::{error, info};

#[async_trait]
pub trait OrbCommandHandler: Send + Sync {
    async fn handle(&self, command: &RecvMessage) -> Result<(), OrbCommandError>;
}

pub struct OrbCommandHandlers {
    handlers: HashMap<String, Box<dyn OrbCommandHandler>>,
}

impl OrbCommandHandlers {
    pub fn init() -> Self {
        let mut handlers = HashMap::new();
        handlers.insert(
            OrbDetailsRequest::type_url(),
            Box::new(cmd_orb_details::OrbDetailsCommandHandler::new())
                as Box<dyn OrbCommandHandler>,
        );
        handlers.insert(
            OrbRebootRequest::type_url(),
            Box::new(cmd_reboot::OrbRebootCommandHandler::new())
                as Box<dyn OrbCommandHandler>,
        );
        Self { handlers }
    }

    pub async fn handle_orb_command(
        &self,
        msg: &RecvMessage,
    ) -> Result<(), OrbCommandError> {
        let any = Any::decode(msg.payload.as_slice()).unwrap();
        let handler = self.handlers.get(&any.type_url);
        match handler {
            Some(handler) => {
                info!("calling handler for command: {:?}", any);
                handler.handle(msg).await
            }
            None => {
                error!("no handler for command: {:?}", any);
                Err(OrbCommandError {
                    error: "no handler for command".to_string(),
                })
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use orb_relay_client::{Amount, Auth, Client, ClientOpts, QoS, SendMessage};
    use orb_relay_messages::{
        orb_commands::v1::OrbDetailsRequest,
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
                error: "nothing".to_string(),
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
    }
}
