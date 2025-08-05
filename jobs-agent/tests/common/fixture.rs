#![allow(dead_code)]
use async_tempfile::TempDir;
use orb_info::OrbId;
use orb_jobs_agent::{
    program::{self, Deps},
    settings::Settings,
    shell::Shell,
};
use orb_relay_client::{Amount, Auth, Client, ClientOpts, SendMessage};
use orb_relay_messages::{
    jobs::v1::{JobExecution, JobExecutionUpdate, JobNotify, JobRequestNext},
    prost::{Message, Name},
    prost_types::Any,
    relay::{
        entity::EntityType, relay_connect_request::Msg, ConnectRequest,
        ConnectResponse, RelayPayload,
    },
};
use orb_relay_test_utils::{IntoRes, TestServer};
use orb_telemetry::TelemetryFlusher;
use std::{collections::VecDeque, time::Duration};
use test_utils::async_bag::AsyncBag;
use tokio::task::{self, JoinHandle};
use uuid::Uuid;

type JobQueue = AsyncBag<VecDeque<JobExecution>>;

pub struct JobAgentFixture {
    _server: TestServer<JobQueue>,
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
        let namespace = "test_namespace".to_string();
        let orb_id = "abcdefff".to_string();
        let orb_id_clone = orb_id.clone();
        let target_service_id = "test_target_service_id".to_string();

        let execution_updates = AsyncBag::new(Vec::new());
        let execution_updates_clone = execution_updates.clone();

        let job_queue: JobQueue = AsyncBag::new(VecDeque::new());
        let job_queue_clone = job_queue.clone();

        let server =
            // a lot of tasks spawned to deal with async -- it a bit cursed
            // but otherwise would require making this a closure that
            // returns a future and that would be a MUCH bigger pain in the ass
            // this also only runs during tests so who cares
            // perf will not be an issue
            TestServer::new(job_queue_clone, move |job_queue, conn_req, clients| {
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
                        if src.clone().unwrap().id == orb_id_clone {
                            // orb is askin for a new job
                            if any.type_url == JobRequestNext::type_url() && JobRequestNext::decode(
                                any.value.as_slice()
                            ).is_ok() {
                                println!("[FLEET-CMDR]: got JobRequestNext from orb!");
                                let job_queue = job_queue.clone();
                                let clients = clients.clone();

                                task::spawn(async move {
                                    let mut job_queue = job_queue.lock().await;

                                    if let Some(job) = job_queue.pop_front() {
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
                                println!("[FLEET-CMDR]: got JobExecutionUpdate from orb!");
                                let execution_updates = execution_updates_clone.clone();
                                task::spawn(async move {
                                    let mut updates = execution_updates.lock().await;
                                    updates.push(update);
                                });
                            }
                        }

                        clients.send(msg);
                        None
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
            .max_connection_attempts(Amount::Val(1))
            .connection_timeout(Duration::from_millis(10))
            .heartbeat(Duration::from_secs(u64::MAX))
            .ack_timeout(Duration::from_millis(10))
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

    pub fn spawn_program(&self, shell: impl Shell + 'static) -> JoinHandle<()> {
        let deps = Deps::new(shell, self.settings.clone());

        task::spawn(async move {
            program::run(deps)
                .await
                .inspect_err(|e| println!("{e}"))
                .unwrap()
        })
    }

    pub async fn enqueue_job(&self, cmd: impl Into<String>) -> String {
        let job_execution_id = Uuid::new_v4().to_string();
        self.enqueue_job_with_id(cmd, job_execution_id).await
    }

    pub async fn enqueue_job_with_id(
        &self,
        cmd: impl Into<String>,
        job_execution_id: impl Into<String>,
    ) -> String {
        let job_execution_id: String = job_execution_id.into();
        let cmd: String = cmd.into();

        let request = JobExecution {
            job_id: cmd.clone(),
            job_execution_id: job_execution_id.clone(),
            job_document: cmd,
            should_cancel: false,
        };

        let mut job_queue = self.job_queue.lock().await;
        job_queue.push_back(request);

        let any = Any {
            type_url: JobNotify::type_url(),
            value: JobNotify::default().encode_to_vec(),
        };

        let payload = Any::from_msg(&any).unwrap().encode_to_vec();

        // send job notify, ENQUEUE job request
        self.client
            .send(
                SendMessage::to(EntityType::Orb)
                    .id(self.settings.orb_id.to_string())
                    .namespace(&self.settings.relay_namespace)
                    .payload(payload),
            )
            .await
            .unwrap();

        job_execution_id
    }
}
