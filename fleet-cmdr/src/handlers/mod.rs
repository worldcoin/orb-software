mod cmd_orb_details;
mod cmd_reboot;

use color_eyre::eyre::{Error, Result};
use orb_relay_messages::fleet_cmdr::v1::{
    JobExecution, JobExecutionStatus, JobExecutionUpdate,
};
use tracing::error;

use crate::job_client::JobClient;

const ORB_DETAILS_COMMAND: &str = "orb_details";
const REBOOT_COMMAND: &str = "reboot";

pub struct OrbCommandHandlers {
    orb_details_handler: cmd_orb_details::OrbDetailsCommandHandler,
    reboot_handler: cmd_reboot::OrbRebootCommandHandler,
}

impl OrbCommandHandlers {
    pub async fn init() -> Self {
        let orb_details_handler =
            cmd_orb_details::OrbDetailsCommandHandler::new().await;
        let reboot_handler = cmd_reboot::OrbRebootCommandHandler::new();
        Self {
            orb_details_handler,
            reboot_handler,
        }
    }

    pub async fn handle_job_execution(
        &self,
        job: &JobExecution,
        job_client: &JobClient,
    ) -> Result<JobExecutionUpdate, Error> {
        let result = match job.job_document.as_str() {
            ORB_DETAILS_COMMAND => self.orb_details_handler.handle(job).await,
            REBOOT_COMMAND => self.reboot_handler.handle(job, job_client).await,
            _ => Ok(JobExecutionUpdate {
                job_id: job.job_id.clone(),
                job_execution_id: job.job_execution_id.clone(),
                status: JobExecutionStatus::Failed as i32,
                std_out: "".to_string(),
                std_err: format!("unknown command: {}", job.job_document),
            }),
        };
        match result {
            Ok(update) => Ok(update),
            Err(e) => {
                error!("error handling job execution: {:?}", e);
                Ok(JobExecutionUpdate {
                    job_id: job.job_id.clone(),
                    job_execution_id: job.job_execution_id.clone(),
                    status: JobExecutionStatus::Failed as i32,
                    std_out: "".to_string(),
                    std_err: e.to_string(),
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
        prost::Message,
        prost_types::Any,
        relay::{
            entity::EntityType, relay_connect_request::Msg, ConnectRequest,
            ConnectResponse,
        },
    };
    use orb_relay_test_utils::{IntoRes, TestServer};
    use tokio::{self, task};

    pub struct NoState;

    pub async fn create_test_server() -> TestServer<NoState> {
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

    pub async fn create_test_client(
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
    async fn test_handle_job_execution() {
        // Arrange
        let sv = create_test_server().await;
        let client_svc =
            create_test_client("test_svc", "test_namespace", EntityType::Service, &sv)
                .await;
        let client_orb =
            create_test_client("test_orb", "test_namespace", EntityType::Orb, &sv)
                .await;
        let job_client_orb =
            JobClient::new(client_orb.clone(), "test_orb", "test_namespace");
        let handlers = OrbCommandHandlers::init().await;

        // Act
        let request = JobExecution {
            job_id: "test_job_id".to_string(),
            job_execution_id: "test_job_execution_id".to_string(),
            job_document: ORB_DETAILS_COMMAND.to_string(),
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
            let any = Any::decode(msg.payload.as_slice()).unwrap();
            let job = JobExecution::decode(any.value.as_slice()).unwrap();
            let result = handlers.handle_job_execution(&job, &job_client_orb).await;
            assert!(result.is_ok());
            let any = Any::from_msg(&result.unwrap()).unwrap();
            msg.reply(any.encode_to_vec(), QoS::AtLeastOnce)
                .await
                .unwrap();
        });

        let result = client_svc.ask(msg).await;
        assert!(result.is_ok());
        let any = Any::decode(result.unwrap().as_slice()).unwrap();
        let response = JobExecutionUpdate::decode(any.value.as_slice());
        assert!(response.is_ok());
    }
}
