//! The agent must reject relay messages from anything other than the configured
//! job server, and keep serving the job server afterwards.

use common::fixture::JobAgentFixture;
use orb_jobs_agent::{
    job_system::{ctx::JobExecutionUpdateExt, handler::JobHandler},
    shell::Host,
};
use orb_relay_client::{Amount, Client, ClientOpts, QoS, SendMessage};
use orb_relay_messages::{
    jobs::v1::JobExecution, prost::Message, prost_types::Any, relay::entity::EntityType,
};
use std::{
    fmt::{self, Write as _},
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};
use tokio::{sync::Notify, task, time};
use tracing::{
    field::{Field, Visit},
    Event, Subscriber,
};
use tracing_subscriber::{
    layer::{Context, SubscriberExt},
    util::SubscriberInitExt,
    Layer,
};

mod common;

const LEGIT_CMD: &str = "ping";

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn rejects_job_execution_from_unexpected_service_id() {
    // Arrange
    let capture = capture();
    let fx = JobAgentFixture::with_namespace("unauthorized_sender_id").await;
    spawn_agent(&fx);

    let attacker = attacker_client(&fx, "unexpected-service", EntityType::Service);

    // Act
    send_job_execution(&attacker, &fx, "exec-unexpected-service").await;
    // positive agent-side signal: it saw the message and rejected the sender
    capture.wait_for_rejection(&["unexpected-service"]).await;

    let legit = fx.enqueue_job(LEGIT_CMD).await;
    time::timeout(Duration::from_secs(60), legit.wait_for_completion())
        .await
        .expect("legit job never completed");

    // Assert: the rejected job never ran; the agent kept serving the server
    assert_only_legit_updates(&fx, &legit.exec_id).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn rejects_job_execution_from_job_server_id_with_wrong_entity_type() {
    // Arrange
    let capture = capture();
    let fx = JobAgentFixture::with_namespace("unauthorized_sender_type").await;
    spawn_agent(&fx);

    // same id and namespace as the job server, so only the entity type differs
    // (a distinct relay routing key, so the fixture's own client stays connected)
    let attacker =
        attacker_client(&fx, &fx.settings.target_service_id, EntityType::App);

    // Act
    send_job_execution(&attacker, &fx, "exec-wrong-entity-type").await;
    capture
        .wait_for_rejection(&[&fx.settings.target_service_id, "App"])
        .await;

    let legit = fx.enqueue_job(LEGIT_CMD).await;
    time::timeout(Duration::from_secs(60), legit.wait_for_completion())
        .await
        .expect("legit job never completed");

    // Assert: the rejected job never ran; the agent kept serving the server
    assert_only_legit_updates(&fx, &legit.exec_id).await;
}

fn spawn_agent(fx: &JobAgentFixture) {
    let deps = fx.deps(Host);

    task::spawn(
        JobHandler::builder()
            .parallel(LEGIT_CMD, async |ctx| Ok(ctx.success().stdout("pong")))
            .build(deps)
            .run(),
    );
}

/// A second relay client, mirroring the fixture's own `ClientOpts`.
fn attacker_client(fx: &JobAgentFixture, id: &str, entity_type: EntityType) -> Client {
    let opts = ClientOpts::entity(entity_type)
        .id(id.to_string())
        .namespace(fx.settings.relay_namespace.clone())
        .endpoint(fx.settings.relay_host.clone())
        .auth(fx.settings.auth.clone())
        .max_connection_attempts(Amount::Val(3))
        .connection_timeout(Duration::from_secs(1))
        .heartbeat(Duration::from_secs(u64::MAX))
        .ack_timeout(Duration::from_secs(1))
        .build();

    let (client, _handle) = Client::connect(opts);

    client
}

async fn send_job_execution(
    client: &Client,
    fx: &JobAgentFixture,
    job_execution_id: &str,
) {
    let job = JobExecution {
        job_id: LEGIT_CMD.to_string(),
        job_execution_id: job_execution_id.to_string(),
        job_document: LEGIT_CMD.to_string(),
        should_cancel: false,
    };

    // the fixture relay decodes every payload as `Any` and panics otherwise
    let payload = Any::from_msg(&job).unwrap().encode_to_vec();

    // bounded so a broken relay ack fails the test instead of hanging it
    time::timeout(
        Duration::from_secs(30),
        client.send(
            SendMessage::to(EntityType::Orb)
                .id(fx.settings.orb_id.to_string())
                .namespace(&fx.settings.relay_namespace)
                .qos(QoS::AtLeastOnce)
                .payload(payload),
        ),
    )
    .await
    .expect("relay send timed out")
    .unwrap();
}

/// Proves the agent reported only the job server's job -- and, since the legit
/// job ran after the rejection, that it kept listening.
async fn assert_only_legit_updates(fx: &JobAgentFixture, legit_exec_id: &str) {
    let updates = fx.execution_updates.read().await;

    assert!(
        !updates.is_empty()
            && updates.iter().all(|u| u.job_execution_id == legit_exec_id),
        "agent acted on a job from an unexpected sender: {updates:?}"
    );
    assert!(updates.iter().any(|u| u.std_out == "pong"));
}

/// Captured `tracing` events, used to observe the agent rejecting a sender.
#[derive(Default)]
struct Capture {
    events: Mutex<Vec<String>>,
    pushed: Notify,
}

impl Capture {
    /// Waits until an event containing every `needle` is captured. Bounded, so a
    /// missing rejection fails the test instead of hanging it.
    async fn wait_for_rejection(&self, needles: &[&str]) {
        let wait = time::timeout(Duration::from_secs(30), async {
            loop {
                // registered before the check so a concurrent push is not missed
                let pushed = self.pushed.notified();

                if self.events.lock().unwrap().iter().any(|e| {
                    e.contains("unexpected sender")
                        && needles.iter().all(|n| e.contains(n))
                }) {
                    return;
                }

                pushed.await;
            }
        });

        assert!(
            wait.await.is_ok(),
            "agent never logged a rejection matching {needles:?}"
        );
    }
}

/// The global subscriber can only be installed once per test binary, and the
/// cases in this file share it. `fixture::init_tracing` is deliberately not used:
/// it installs the orb-telemetry subscriber instead.
fn capture() -> &'static Arc<Capture> {
    static CAPTURE: OnceLock<Arc<Capture>> = OnceLock::new();

    CAPTURE.get_or_init(|| {
        let capture = Arc::new(Capture::default());

        tracing_subscriber::registry()
            .with(CaptureLayer(capture.clone()))
            .init();

        capture
    })
}

struct CaptureLayer(Arc<Capture>);

impl<S: Subscriber> Layer<S> for CaptureLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut fields = String::new();
        event.record(&mut FieldWriter(&mut fields));

        self.0.events.lock().unwrap().push(fields);
        self.0.pushed.notify_waiters();
    }
}

struct FieldWriter<'a>(&'a mut String);

impl Visit for FieldWriter<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        let _ = write!(self.0, "{}={:?} ", field.name(), value);
    }
}
