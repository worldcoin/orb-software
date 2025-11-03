#![allow(dead_code)]
use super::job_queue::{self, JobQueue};
use async_tempfile::TempDir;
use orb_info::OrbId;
use orb_jobs_agent::{
    program::{self, Deps},
    settings::Settings,
    shell::Shell,
};
use orb_relay_client::{Amount, Auth, Client, ClientOpts, QoS, SendMessage};
use orb_relay_messages::{
    jobs::v1::{
        JobCancel, JobExecution, JobExecutionStatus, JobExecutionUpdate, JobNotify,
        JobRequestNext,
    },
    prost::{Message, Name},
    prost_types::Any,
    relay::{
        entity::EntityType, relay_connect_request::Msg, Ack, ConnectRequest,
        ConnectResponse, RelayPayload,
    },
};
use orb_relay_test_utils::{IntoRes, TestServer};
use orb_telemetry::TelemetryFlusher;
use std::time::Duration;
use test_utils::async_bag::AsyncBag;
use tokio::task::{self, JoinHandle};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// A fixture for testing `orb-jobs-agent`.
/// - Spawns a fake server equivalent to fleet-cmdr, used to enqueue job requests.
/// - Holds all received `JobExecutionUpdate` reeived from `orb-jobs-agent` for testing assertions.
/// - Allows easy spawning of new `orb-jobs-agent` programs.
pub struct JobAgentFixture {
    _server: TestServer<()>,
    client: Client,
    pub settings: Settings,
    pub execution_updates: AsyncBag<Vec<JobExecutionUpdate>>,
    pub job_queue: JobQueue,
    _tempdir: TempDir,
}

impl JobAgentFixture {
    pub fn init_tracing(&self) -> TelemetryFlusher {
        orb_telemetry::TelemetryConfig::new().init()
    }

    pub async fn new() -> Self {
        Self::with_namespace("default-test-namespace").await
    }

    pub async fn with_namespace(namespace: impl Into<String>) -> Self {
        let namespace = namespace.into();
        let orb_id = "ba11ba11".to_string();
        let orb_id_clone = orb_id.clone();
        let target_service_id = "fleet-cmdr".to_string();

        let execution_updates = AsyncBag::new(Vec::new());
        let execution_updates_clone = execution_updates.clone();

        let job_queue = JobQueue::default();
        let jqueue = job_queue.clone();

        let server =
            // a lot of tasks spawned to deal with async -- it a bit cursed
            // but otherwise would require making this a closure that
            // returns a future and that would be a MUCH bigger pain in the ass
            // this also only runs during tests so who cares
            // perf will not be an issue
            TestServer::new((), move |_, conn_req, clients| {
                match conn_req {
                    Msg::ConnectRequest(ConnectRequest { client_id, .. }) => {
                        ConnectResponse {
                            client_id: client_id.unwrap().id.clone(),
                            success: true,
                            error: "Nothing".to_string(),
                        }
                        .into_res()
                    }

                    Msg::Payload(msg) => {
                        // if message comes from the orb, its must be going to the server
                        // we add them to the execution_update and completion bags
                        // for inspecting during tests
                        let src = msg.src.clone();
                        let dst = msg.dst.clone();
                        let seq = msg.seq;

                        let any = Any::decode(msg.payload.clone().unwrap().value.as_slice()).unwrap();
                        println!("[RELAY] src {}, dst {}, type_url: {}", src.as_ref().unwrap().id, dst.as_ref().unwrap().id, any.type_url);
                        if src.clone().unwrap().id == orb_id_clone {
                            // orb is asking for a new job
                            if any.type_url == JobRequestNext::type_url() && let Ok(req) = JobRequestNext::decode(
                                any.value.as_slice()
                            ) {
                                let jqueue = jqueue.clone();
                                let clients = clients.clone();

                                task::spawn(async move {
                                    if let Some(job) = jqueue.next(&req.ignore_job_execution_ids).await {
                                        let any = Any {
                                           type_url: JobExecution::type_url(),
                                           value: job.encode_to_vec(),
                                        };

                                        let payload = RelayPayload {
                                            src: dst,
                                            dst: src,
                                            seq,
                                            payload: Some(Any::from_msg(&any).unwrap()),
                                        };

                                        clients.send(payload);
                                    }
                                });
                            } else if any.type_url == JobExecutionUpdate::type_url() && let Ok(update) = JobExecutionUpdate::decode(
                                any.value.as_slice()
                            ) {
                                let execution_updates = execution_updates_clone.clone();
                                let jqueue = jqueue.clone();
                                task::spawn(async move {
                                    match JobExecutionStatus::try_from(update.status).unwrap() {
                                        | JobExecutionStatus::Succeeded
                                        | JobExecutionStatus::Failed
                                        | JobExecutionStatus::Cancelled
                                        | JobExecutionStatus::FailedUnsupported => {
                                            jqueue.handled(&update.job_execution_id).await;
                                        }
                                        _ => (),
                                    };

                                    let mut updates = execution_updates.lock().await;
                                    updates.push(update);
                                }); }
                        }

                        clients.send(msg);

                        Ack { seq }.into_res()
                    }

                    _ => None,
                }
            })
            .await;

        let relay_host = format!("http://{}", server.addr());
        let auth = Auth::Token(Default::default());

        let opts = ClientOpts::entity(EntityType::Service)
            .id(target_service_id.clone())
            .namespace(namespace.clone())
            .endpoint(relay_host.clone())
            .auth(auth.clone())
            .max_connection_attempts(Amount::Val(3))
            .connection_timeout(Duration::from_secs(1))
            .heartbeat(Duration::from_secs(u64::MAX))
            .ack_timeout(Duration::from_secs(1))
            .build();

        // this is the client used by the fleet commander
        let (client, _handle) = Client::connect(opts);

        let tempdir = TempDir::new().await.unwrap();
        let settings = Settings {
            orb_id: OrbId::Short(orb_id.parse().unwrap()),
            auth,
            relay_host,
            relay_namespace: namespace,
            target_service_id: target_service_id.to_string(),
            store_path: tempdir.to_path_buf(),
            // Use non-existent paths by default for tests (can be overridden)
            calibration_file_path: "/nonexistent/calibration.json".into(),
            os_release_path: "/nonexistent/os-release".into(),
            versions_file_path: "/nonexistent/versions.json".into(),
        };

        Self {
            _server: server,
            client,
            settings,
            execution_updates,
            job_queue,
            _tempdir: tempdir,
        }
    }

    pub fn spawn_program(&self, shell: impl Shell + 'static) -> ProgramHandle {
        let deps = Deps::new(shell, self.settings.clone());
        let cancel_token = CancellationToken::new();
        let cancel_token_clone = cancel_token.clone();

        let join_handle = task::spawn(async move {
            tokio::select! {
                r = program::run(deps) => {
                    if let Err(e) = r {
                        println!("program::run failed with {e}");
                    }
                }

                _ = cancel_token_clone.cancelled() => (),
            };
        });

        ProgramHandle {
            cancel_token,
            join_handle,
        }
    }

    pub async fn enqueue_job(&self, cmd: impl Into<String>) -> job_queue::Ticket {
        let job_execution_id = Uuid::new_v4().to_string();
        self.enqueue_job_with_id(cmd, job_execution_id).await
    }

    pub async fn enqueue_job_with_id(
        &self,
        cmd: impl Into<String>,
        job_execution_id: impl Into<String>,
    ) -> job_queue::Ticket {
        let job_execution_id: String = job_execution_id.into();
        let cmd: String = cmd.into();

        let request = JobExecution {
            job_id: cmd.clone(),
            job_execution_id: job_execution_id.clone(),
            job_document: cmd,
            should_cancel: false,
        };

        let ticket = self.job_queue.enqueue(request).await;

        let payload = Any::from_msg(&JobNotify::default())
            .unwrap()
            .encode_to_vec();

        // send job notify, ENQUEUE job request
        self.client
            .send(
                SendMessage::to(EntityType::Orb)
                    .id(self.settings.orb_id.to_string())
                    .namespace(&self.settings.relay_namespace)
                    .qos(QoS::AtLeastOnce)
                    .payload(payload),
            )
            .await
            .unwrap();

        ticket
    }

    pub async fn cancel_job(&self, job_execution_id: impl Into<String>) {
        let req = JobCancel {
            job_execution_id: job_execution_id.into(),
        };

        let payload = Any::from_msg(&req).unwrap().encode_to_vec();

        self.client
            .send(
                SendMessage::to(EntityType::Orb)
                    .id(self.settings.orb_id.to_string())
                    .namespace(&self.settings.relay_namespace)
                    .qos(QoS::AtLeastOnce)
                    .payload(payload),
            )
            .await
            .unwrap();
    }
}

pub struct ProgramHandle {
    join_handle: JoinHandle<()>,
    cancel_token: CancellationToken,
}

impl ProgramHandle {
    /// Stops gracefully
    pub async fn stop(self) {
        self.cancel_token.cancel();
        self.join_handle.await.unwrap();
    }

    /// Stops forcefully aborting the `task::JoinHandle`
    pub fn abort(self) {
        self.cancel_token.cancel();
        self.join_handle.abort();
    }
}
