mod cmd_orb_details;
mod cmd_read_file;
mod cmd_reboot;
mod cmd_run_binary;
mod cmd_tail_logs;

use color_eyre::eyre::{Error, Result};
use orb_relay_messages::jobs::v1::{
    JobExecution, JobExecutionStatus, JobExecutionUpdate,
};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use tracing::error;

use crate::job_client::JobClient;
use crate::orchestrator::JobCompletion;

const CHECK_MY_ORB_COMMAND: &str = "check_my_orb";
const MCU_INFO_COMMAND: &str = "mcu_info";
const ORB_DETAILS_COMMAND: &str = "orb_details";
const READ_GIMBAL_CALIBRATION_FILE: &str = "read_gimbal";
const REBOOT_COMMAND: &str = "reboot";
const TAIL_CORE_LOGS_COMMAND: &str = "tail_core_logs";
const TAIL_TEST_COMMAND: &str = "tail_test";

/// JobHandler trait for the hybrid orchestrator pattern
///
/// This trait defines the contract for all job handlers in the system:
///
/// ## Architecture Pattern:
/// - **Orchestrator** owns job lifecycle (requesting, tracking, cancellation)
/// - **Handlers** own job execution (updates, domain logic, completion signaling)
///
/// ## Handler Responsibilities:
/// - Execute the job logic
/// - Send progress updates via job_client
/// - Signal completion via completion_tx
/// - Respect cancellation via cancel_token
///
/// ## Orchestrator Responsibilities:
/// - Request jobs from the relay
/// - Track active jobs for cancellation
/// - Manage parallelization rules
/// - Request next job when current job completes
///
/// ## Handler Implementation Patterns:
///
/// **Immediate handlers** (like read_file, orb_details):
/// - Execute work synchronously
/// - Send single job update with result
/// - Signal completion immediately
/// - Handle cancellation before starting work
///
/// **Background handlers** (like tail_logs, run_binary):
/// - Spawn background task for long-running work
/// - Send initial InProgress update
/// - Send progress updates as work continues
/// - Monitor cancellation token throughout execution
/// - Send final completion signal when done
#[allow(async_fn_in_trait)]
pub trait JobHandler {
    /// Handle a job execution
    ///
    /// # Arguments
    /// * `job` - The job to execute
    /// * `job_client` - Client for sending job updates
    /// * `completion_tx` - Channel to signal job completion (must be called exactly once)
    /// * `cancel_token` - Token for job cancellation (handler should monitor this)
    ///
    /// # Returns
    /// Result indicating whether the handler setup succeeded (not job execution success)
    async fn handle(
        &self,
        job: &JobExecution,
        job_client: &JobClient,
        completion_tx: oneshot::Sender<JobCompletion>,
        cancel_token: CancellationToken,
    ) -> Result<(), Error>;
}

#[derive(Clone)]
pub struct OrbCommandHandlers {
    orb_details_handler: cmd_orb_details::OrbDetailsCommandHandler,
    read_file_handler: cmd_read_file::ReadFileCommandHandler,
    reboot_handler: cmd_reboot::OrbRebootCommandHandler,
    run_binary_handler: cmd_run_binary::RunBinaryCommandHandler,
    tail_logs_handler: cmd_tail_logs::TailLogsCommandHandler,
}

impl OrbCommandHandlers {
    pub async fn init() -> Self {
        let read_file_handler = cmd_read_file::ReadFileCommandHandler::new();
        let reboot_handler = cmd_reboot::OrbRebootCommandHandler::new();
        let run_binary_handler = cmd_run_binary::RunBinaryCommandHandler::new();
        let orb_details_handler = cmd_orb_details::OrbDetailsCommandHandler::new();
        let tail_logs_handler = cmd_tail_logs::TailLogsCommandHandler::new();
        Self {
            read_file_handler,
            run_binary_handler,
            orb_details_handler,
            reboot_handler,
            tail_logs_handler,
        }
    }

    /// Handle a job execution using the new hybrid orchestrator pattern
    ///
    /// This method routes jobs to the appropriate handler based on job_document.
    /// Each handler is responsible for:
    /// - Executing the job logic
    /// - Sending progress updates
    /// - Signaling completion
    /// - Respecting cancellation
    pub async fn handle_job_execution(
        &self,
        job: &JobExecution,
        job_client: &JobClient,
        completion_tx: oneshot::Sender<JobCompletion>,
        cancel_token: CancellationToken,
    ) -> Result<(), Error> {
        // Route to appropriate handler based on job type
        match job.job_document.as_str() {
            CHECK_MY_ORB_COMMAND => {
                self.run_binary_handler
                    .handle_binary(
                        job,
                        job_client,
                        completion_tx,
                        cancel_token,
                        "/usr/local/bin/check-my-orb",
                        &vec![],
                    )
                    .await
            }
            ORB_DETAILS_COMMAND => {
                self.orb_details_handler
                    .handle(job, job_client, completion_tx, cancel_token)
                    .await
            }
            MCU_INFO_COMMAND => {
                self.run_binary_handler
                    .handle_binary(
                        job,
                        job_client,
                        completion_tx,
                        cancel_token,
                        "/usr/local/bin/orb-mcu-util",
                        &vec!["info".to_string()],
                    )
                    .await
            }
            READ_GIMBAL_CALIBRATION_FILE => {
                self.read_file_handler
                    .handle_file(
                        job,
                        job_client,
                        completion_tx,
                        cancel_token,
                        "/usr/persistent/calibration.json",
                    )
                    .await
            }
            REBOOT_COMMAND => {
                self.reboot_handler
                    .handle(job, job_client, completion_tx, cancel_token)
                    .await
            }
            TAIL_CORE_LOGS_COMMAND => {
                self.tail_logs_handler
                    .handle_logs(
                        job,
                        job_client,
                        completion_tx,
                        cancel_token,
                        "worldcoin-core",
                    )
                    .await
            }
            TAIL_TEST_COMMAND => {
                self.tail_logs_handler
                    .handle_logs(job, job_client, completion_tx, cancel_token, "test")
                    .await
            }
            _ => {
                // Unknown command - send unsupported failure update and complete
                let update = JobExecutionUpdate {
                    job_id: job.job_id.clone(),
                    job_execution_id: job.job_execution_id.clone(),
                    status: JobExecutionStatus::FailedUnsupported as i32,
                    std_out: String::new(),
                    std_err: format!("unsupported command: {}", job.job_document),
                };

                if let Err(e) = job_client.send_job_update(&update).await {
                    error!(
                        "Failed to send job update for unsupported command: {:?}",
                        e
                    );
                }

                completion_tx
                    .send(JobCompletion::failure(
                        job.job_execution_id.clone(),
                        format!("unsupported command: {}", job.job_document),
                    ))
                    .ok();

                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::orchestrator::JobRegistry;
    use orb_relay_client::{Amount, Auth, Client, ClientOpts};
    use orb_relay_messages::relay::{
        entity::EntityType, relay_connect_request::Msg, ConnectRequest, ConnectResponse,
    };
    use orb_relay_test_utils::{IntoRes, TestServer};
    use tokio;

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
    async fn test_handle_job_execution_orb_details() {
        // Arrange
        let sv = create_test_server().await;
        let _client_svc =
            create_test_client("test_svc", "test_namespace", EntityType::Service, &sv)
                .await;
        let client_orb =
            create_test_client("test_orb", "test_namespace", EntityType::Orb, &sv)
                .await;
        let job_client_orb = JobClient::new(
            client_orb.clone(),
            "test_orb",
            "test_namespace",
            JobRegistry::new(),
            crate::orchestrator::JobConfig::new(),
        );
        let handlers = OrbCommandHandlers::init().await;

        // Act
        let request = JobExecution {
            job_id: "test_job_id".to_string(),
            job_execution_id: "test_job_execution_id".to_string(),
            job_document: ORB_DETAILS_COMMAND.to_string(),
            should_cancel: false,
        };

        let (completion_tx, completion_rx) = oneshot::channel();
        let cancel_token = CancellationToken::new();

        // Start the handler
        let result = handlers
            .handle_job_execution(
                &request,
                &job_client_orb,
                completion_tx,
                cancel_token,
            )
            .await;
        assert!(result.is_ok());

        // Wait for completion
        let completion = completion_rx.await.unwrap();
        assert_eq!(completion.status, JobExecutionStatus::Succeeded);
        assert_eq!(completion.job_execution_id, "test_job_execution_id");
    }

    #[tokio::test]
    async fn test_handle_job_execution_unsupported_command() {
        // Arrange
        let sv = create_test_server().await;
        let _client_svc =
            create_test_client("test_svc", "test_namespace", EntityType::Service, &sv)
                .await;
        let client_orb =
            create_test_client("test_orb", "test_namespace", EntityType::Orb, &sv)
                .await;
        let job_client_orb = JobClient::new(
            client_orb.clone(),
            "test_orb",
            "test_namespace",
            JobRegistry::new(),
            crate::orchestrator::JobConfig::new(),
        );
        let handlers = OrbCommandHandlers::init().await;

        // Act
        let request = JobExecution {
            job_id: "test_job_id".to_string(),
            job_execution_id: "test_job_execution_id".to_string(),
            job_document: "unsupported_command".to_string(), // Unknown command
            should_cancel: false,
        };

        let (completion_tx, completion_rx) = oneshot::channel();
        let cancel_token = CancellationToken::new();

        // Start the handler
        let result = handlers
            .handle_job_execution(
                &request,
                &job_client_orb,
                completion_tx,
                cancel_token,
            )
            .await;
        assert!(result.is_ok());

        // Wait for completion
        let completion = completion_rx.await.unwrap();
        assert_eq!(completion.status, JobExecutionStatus::Failed); // Should be Failed in completion
        assert_eq!(completion.job_execution_id, "test_job_execution_id");
        assert!(completion
            .final_message
            .as_ref()
            .unwrap()
            .contains("unsupported command"));
    }
}
