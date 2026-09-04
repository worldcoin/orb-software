use crate::job_system::{
    orchestrator::{JobConfig, JobRegistry},
    sanitize::redact_job_document,
};
use color_eyre::eyre::{eyre, Result};
use orb_relay_client::{Client, QoS, SendMessage};
use orb_relay_messages::{
    jobs::v1::{
        JobCancel, JobExecution, JobExecutionUpdate, JobNotify, JobRequestNext,
    },
    prost::{DecodeError, Message, Name},
    prost_types::Any,
    relay::{entity::EntityType, Entity},
};
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::{error, info, warn};

/// Sender labels are attacker controlled and unbounded; truncate before logging.
const MAX_LOGGED_LABEL_CHARS: usize = 128;

/// Log the first few unexpected senders, then only every
/// `UNEXPECTED_SENDER_LOG_EVERY`-th one, so warn volume never scales 1:1 with
/// unsolicited traffic. NOTE: the counter is global, not per-sender -- a noisy
/// unexpected sender can delay first-log evidence of a second distinct one;
/// accepted for the shadow window (a per-sender map would grow unbounded on
/// attacker-chosen keys).
const UNEXPECTED_SENDER_LOG_BURST: u64 = 10;
const UNEXPECTED_SENDER_LOG_EVERY: u64 = 100;

/// Process-local count of inbound messages from unexpected senders.
static UNEXPECTED_SENDERS: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
pub struct JobClient {
    relay_client: Client,
    target_service_id: String,
    relay_namespace: String,
    job_registry: JobRegistry,
    job_config: JobConfig,
}

impl JobClient {
    pub fn new(
        relay_client: Client,
        target_service_id: &str,
        relay_namespace: &str,
        job_registry: JobRegistry,
        job_config: JobConfig,
    ) -> Self {
        Self {
            relay_client,
            target_service_id: target_service_id.to_string(),
            relay_namespace: relay_namespace.to_string(),
            job_registry,
            job_config,
        }
    }

    pub async fn listen_for_job(&self) -> Result<JobExecution, orb_relay_client::Err> {
        loop {
            let msg = match self.relay_client.recv().await {
                Ok(msg) => msg,
                Err(e) => {
                    error!("error receiving from relay: {:?}", e);
                    return Err(e);
                }
            };

            // Only the configured job server may drive job execution, cancellation,
            // or request-next; messages from any other sender are dropped before
            // their payload is decoded.
            if !is_authorized_sender(
                &msg.from,
                &self.target_service_id,
                &self.relay_namespace,
            ) {
                log_unexpected_sender(&msg.from);
                continue;
            }

            match classify_inbound(&msg.payload) {
                InboundDecision::Notify(job_notify) => {
                    info!("received JobNotify: {:?}", job_notify);
                    let _ = self.request_next_job().await;
                }

                InboundDecision::Execution(job) => {
                    info!(
                        job_id = %job.job_id,
                        job_execution_id = %job.job_execution_id,
                        job_document = %redact_job_document(&job.job_document),
                        should_cancel = job.should_cancel,
                        "received JobExecution"
                    );
                    return Ok(job);
                }

                InboundDecision::Cancel(job_cancel) => {
                    info!(
                        job_execution_id = %job_cancel.job_execution_id,
                        "received JobCancel"
                    );
                    let cancelled = self
                        .job_registry
                        .cancel_job(&job_cancel.job_execution_id)
                        .await;
                    if cancelled {
                        info!(
                            job_execution_id = %job_cancel.job_execution_id,
                            "Successfully cancelled job"
                        );
                    } else {
                        warn!(
                            job_execution_id = %job_cancel.job_execution_id,
                            "Attempted to cancel non-existent or already completed job"
                        );
                    }
                }

                InboundDecision::Undecodable { msg_name, err } => {
                    error!("error decoding {}: {:?}", msg_name, err);
                }

                InboundDecision::UnknownType(type_url) => {
                    error!("received unexpected message type: {:?}", type_url);
                }
            }
        }
    }

    /// Requests for a next job to be run, excluding the ones that are
    /// currently running (determined by `running_job_execution_ids` arg)
    pub async fn request_next_job(&self) -> Result<(), orb_relay_client::Err> {
        let mut running_ids = self.job_registry.get_active_job_ids().await;
        let mut completed_ids = self.job_registry.get_completed_job_ids().await;

        running_ids.append(&mut completed_ids);
        let job_ids_to_ignore = running_ids;

        let job_request = JobRequestNext {
            ignore_job_execution_ids: job_ids_to_ignore.clone(),
        };

        let any = Any::from_msg(&job_request).unwrap();
        self.relay_client
            .send(
                SendMessage::to(EntityType::Service)
                    .id(self.target_service_id.clone())
                    .namespace(self.relay_namespace.clone())
                    .qos(QoS::AtLeastOnce)
                    .payload(any.encode_to_vec()),
            )
            .await?;

        info!(
            "sent JobRequestNext ignoring {} job execution IDs: {:?}",
            job_ids_to_ignore.len(),
            job_ids_to_ignore
        );

        Ok(())
    }

    /// Check if we should request more jobs and do so if appropriate
    /// This method is used to implement parallel job execution
    /// Returns `false` if no jobs were requested.
    pub async fn try_request_more_jobs(&self) -> Result<bool, orb_relay_client::Err> {
        // Check if we should request more jobs based on current configuration
        if !self
            .job_config
            .should_request_more_jobs(&self.job_registry)
            .await
        {
            return Ok(false);
        }

        // Request next job with current running job IDs
        self.request_next_job()
            .await
            .inspect_err(|e| error!("Failed to request additional job: {:?}", e))?;

        info!("Successfully requested additional job for parallel execution");

        Ok(true)
    }

    pub async fn send_job_update(
        &self,
        job_update: &JobExecutionUpdate,
    ) -> Result<(), orb_relay_client::Err> {
        info!(
            job_execution_id = %job_update.job_execution_id,
            job_id = %job_update.job_id,
            "sending job update: {:?}",
            job_update
        );
        let any = Any::from_msg(job_update).unwrap();
        self.relay_client
            .send(
                SendMessage::to(EntityType::Service)
                    .id(self.target_service_id.clone())
                    .namespace(self.relay_namespace.clone())
                    .qos(QoS::AtLeastOnce)
                    .payload(any.encode_to_vec()),
            )
            .await
            .inspect_err(|e| {
                error!(
                    job_execution_id = %job_update.job_execution_id,
                    job_id = %job_update.job_id,
                    "error sending JobExecutionUpdate: {:?}",
                    e
                )
            })?;

        info!(
            job_execution_id = %job_update.job_execution_id,
            job_id = %job_update.job_id,
            "sent JobExecutionUpdate"
        );

        Ok(())
    }

    pub async fn force_relay_reconnect(&self) -> Result<()> {
        self.relay_client
            .reconnect()
            .await
            .map_err(|_| eyre!("failed to force reconnect orb relay"))
    }
}

/// What the inbound step decided about a single relay message.
#[derive(Debug)]
enum InboundDecision {
    Notify(JobNotify),
    Execution(JobExecution),
    Cancel(JobCancel),
    Undecodable {
        msg_name: &'static str,
        err: DecodeError,
    },
    UnknownType(String),
}

/// Whether `from` matches the configured job server.
///
/// `from` is a sender-supplied label rather than verified identity (authenticity
/// is enforced by the relay server), so this is defense-in-depth against any
/// entity other than the configured job server reaching the decoders and the
/// side effects behind them (job execution, cancellation, request-next).
fn is_authorized_sender(
    from: &Entity,
    target_service_id: &str,
    relay_namespace: &str,
) -> bool {
    // prost exposes `entity_type` as a raw i32; compare the same way the relay
    // client itself does.
    EntityType::try_from(from.entity_type) == Ok(EntityType::Service)
        && from.id == target_service_id
        && from.namespace == relay_namespace
}

/// Decodes an inbound relay message and classifies it by payload type.
fn classify_inbound(payload: &[u8]) -> InboundDecision {
    let any = match Any::decode(payload) {
        Ok(any) => any,
        Err(err) => {
            return InboundDecision::Undecodable {
                msg_name: "message",
                err,
            }
        }
    };

    if any.type_url == JobNotify::type_url() {
        match JobNotify::decode(any.value.as_slice()) {
            Ok(job_notify) => InboundDecision::Notify(job_notify),
            Err(err) => InboundDecision::Undecodable {
                msg_name: "JobNotify",
                err,
            },
        }
    } else if any.type_url == JobExecution::type_url() {
        match JobExecution::decode(any.value.as_slice()) {
            Ok(job) => InboundDecision::Execution(job),
            Err(err) => InboundDecision::Undecodable {
                msg_name: "JobExecution",
                err,
            },
        }
    } else if any.type_url == JobCancel::type_url() {
        match JobCancel::decode(any.value.as_slice()) {
            Ok(job_cancel) => InboundDecision::Cancel(job_cancel),
            Err(err) => InboundDecision::Undecodable {
                msg_name: "JobCancel",
                err,
            },
        }
    } else {
        InboundDecision::UnknownType(any.type_url)
    }
}

/// Logs a rejected message -- never its payload -- and only for the first few
/// occurrences plus every `UNEXPECTED_SENDER_LOG_EVERY`-th one thereafter.
fn log_unexpected_sender(from: &Entity) {
    let unexpected_total = UNEXPECTED_SENDERS.fetch_add(1, Ordering::Relaxed) + 1;

    if unexpected_total > UNEXPECTED_SENDER_LOG_BURST
        && !unexpected_total.is_multiple_of(UNEXPECTED_SENDER_LOG_EVERY)
    {
        return;
    }

    warn!(
        // `?` so control characters in the attacker-chosen labels are escaped
        sender_id = ?truncate_label(&from.id),
        sender_namespace = ?truncate_label(&from.namespace),
        sender_entity_type = ?EntityType::try_from(from.entity_type),
        unexpected_total,
        "rejected relay message from unexpected sender"
    );
}

/// Truncates by chars: byte slicing would panic mid-codepoint and let a sender
/// take down the recv loop.
fn truncate_label(label: &str) -> String {
    label.chars().take(MAX_LOGGED_LABEL_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use orb_relay_messages::jobs::v1::{
        JobExecution, JobExecutionStatus, JobExecutionUpdate,
    };

    #[test]
    fn test_job_execution_update_creation_for_cancellation() {
        // Test that we can create the correct JobExecutionUpdate for cancellation
        let job_execution = JobExecution {
            job_id: "test_job_123".to_string(),
            job_execution_id: "test_execution_456".to_string(),
            job_document: "orb_details".to_string(),
            should_cancel: true,
        };

        // Create the update that main.rs would create for should_cancel = true
        let cancel_update = JobExecutionUpdate {
            job_id: job_execution.job_id.clone(),
            job_execution_id: job_execution.job_execution_id.clone(),
            status: JobExecutionStatus::Failed as i32,
            std_out: String::new(),
            std_err: "Job was cancelled".to_string(),
        };

        // Verify the update has the correct fields
        assert_eq!(cancel_update.job_id, "test_job_123");
        assert_eq!(cancel_update.job_execution_id, "test_execution_456");
        assert_eq!(cancel_update.status, JobExecutionStatus::Failed as i32);
        assert_eq!(cancel_update.std_err, "Job was cancelled");
        assert_eq!(cancel_update.std_out, "");
    }

    #[test]
    fn test_should_cancel_field_detection() {
        // Test that we can properly detect should_cancel field
        let normal_job = JobExecution {
            job_id: "job1".to_string(),
            job_execution_id: "exec1".to_string(),
            job_document: "orb_details".to_string(),
            should_cancel: false,
        };

        let cancelled_job = JobExecution {
            job_id: "job2".to_string(),
            job_execution_id: "exec2".to_string(),
            job_document: "orb_details".to_string(),
            should_cancel: true,
        };

        assert!(
            !normal_job.should_cancel,
            "Normal job should not be cancelled"
        );
        assert!(
            cancelled_job.should_cancel,
            "Cancelled job should be marked as cancelled"
        );
    }

    #[test]
    fn test_job_request_with_ignore_ids() {
        // Test creating JobRequestNext with ignore IDs directly
        let ignore_ids = vec![
            "job_exec_1".to_string(),
            "job_exec_2".to_string(),
            "job_exec_3".to_string(),
        ];

        let job_request = JobRequestNext {
            ignore_job_execution_ids: ignore_ids.clone(),
        };

        assert_eq!(job_request.ignore_job_execution_ids, ignore_ids);
        assert_eq!(job_request.ignore_job_execution_ids.len(), 3);

        // Test with empty IDs
        let empty_request = JobRequestNext {
            ignore_job_execution_ids: vec![],
        };

        assert!(empty_request.ignore_job_execution_ids.is_empty());
    }

    #[test]
    fn test_default_job_request() {
        // Test that default JobRequestNext has empty ignore_job_execution_ids
        let default_request = JobRequestNext::default();
        assert!(default_request.ignore_job_execution_ids.is_empty());
    }

    const TARGET_SERVICE_ID: &str = "fleet-cmdr";
    const RELAY_NAMESPACE: &str = "test-namespace";

    fn job_server() -> Entity {
        Entity {
            id: TARGET_SERVICE_ID.to_string(),
            entity_type: EntityType::Service as i32,
            namespace: RELAY_NAMESPACE.to_string(),
        }
    }

    fn execution_payload() -> Vec<u8> {
        Any::from_msg(&JobExecution {
            job_id: "job".to_string(),
            job_execution_id: "exec".to_string(),
            job_document: "orb_details".to_string(),
            should_cancel: false,
        })
        .unwrap()
        .encode_to_vec()
    }

    fn classify(payload: &[u8]) -> InboundDecision {
        classify_inbound(payload)
    }

    fn authorized(from: &Entity) -> bool {
        is_authorized_sender(from, TARGET_SERVICE_ID, RELAY_NAMESPACE)
    }

    #[test]
    fn job_server_execution_is_accepted() {
        let decision = classify(&execution_payload());

        let InboundDecision::Execution(job) = decision else {
            panic!("expected Execution, got {decision:?}");
        };
        assert_eq!(job.job_execution_id, "exec");
    }

    #[test]
    fn job_server_notify_is_accepted() {
        let payload = Any::from_msg(&JobNotify::default())
            .unwrap()
            .encode_to_vec();

        assert!(matches!(classify(&payload), InboundDecision::Notify(_)));
    }

    #[test]
    fn job_server_cancel_is_accepted() {
        let payload = Any::from_msg(&JobCancel {
            job_execution_id: "exec".to_string(),
        })
        .unwrap()
        .encode_to_vec();

        let decision = classify(&payload);

        let InboundDecision::Cancel(cancel) = decision else {
            panic!("expected Cancel, got {decision:?}");
        };
        assert_eq!(cancel.job_execution_id, "exec");
    }

    #[test]
    fn job_server_garbage_payload_is_undecodable() {
        assert!(matches!(
            classify(&[0xff, 0xff, 0xff, 0xff]),
            InboundDecision::Undecodable { .. }
        ));
    }

    /// Unauthorized senders are rejected by `listen_for_job` before their
    /// payload is decoded.
    #[test]
    fn unexpected_senders_are_rejected() {
        let unexpected = [
            (
                "wrong id",
                Entity {
                    id: "unexpected-service".to_string(),
                    ..job_server()
                },
            ),
            (
                "wrong namespace",
                Entity {
                    namespace: "other-namespace".to_string(),
                    ..job_server()
                },
            ),
            (
                "app",
                Entity {
                    entity_type: EntityType::App as i32,
                    ..job_server()
                },
            ),
            (
                "orb",
                Entity {
                    entity_type: EntityType::Orb as i32,
                    ..job_server()
                },
            ),
            (
                "unspecified",
                Entity {
                    entity_type: EntityType::Unspecified as i32,
                    ..job_server()
                },
            ),
            (
                "out of range entity type",
                Entity {
                    entity_type: 42,
                    ..job_server()
                },
            ),
        ];

        assert!(authorized(&job_server()), "the job server itself must pass");
        for (case, from) in unexpected {
            assert!(
                !authorized(&from),
                "{case}: expected the sender to be flagged as unauthorized"
            );
        }
    }

    #[test]
    fn logged_sender_labels_are_truncated_char_safely() {
        // multi-byte chars: byte slicing at 128 would panic mid-codepoint
        let label = "é".repeat(200);
        let truncated = truncate_label(&label);

        assert_eq!(truncated.chars().count(), MAX_LOGGED_LABEL_CHARS);
    }
}
