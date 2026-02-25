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
    prost::{Message, Name},
    prost_types::Any,
    relay::entity::EntityType,
};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

#[derive(Debug, Clone)]
pub(crate) enum JobTransport {
    Relay {
        relay_client: Client,
        target_service_id: String,
        relay_namespace: String,
    },
    Local(LocalTransport),
}

#[derive(Debug, Clone)]
pub(crate) struct LocalTransport {
    pending_job: Arc<Mutex<Option<JobExecution>>>,
    final_status: Arc<Mutex<Option<i32>>>,
    shutdown: CancellationToken,
}

#[derive(Debug, Clone)]
pub struct JobClient {
    transport: JobTransport,
    job_registry: JobRegistry,
    job_config: JobConfig,
}

impl JobClient {
    pub(crate) fn new(
        transport: JobTransport,
        job_registry: JobRegistry,
        job_config: JobConfig,
    ) -> Self {
        Self {
            transport,
            job_registry,
            job_config,
        }
    }
}

impl JobTransport {
    pub(crate) fn service(
        relay_client: Client,
        target_service_id: &str,
        relay_namespace: &str,
    ) -> Self {
        Self::Relay {
            relay_client,
            target_service_id: target_service_id.to_string(),
            relay_namespace: relay_namespace.to_string(),
        }
    }

    pub(crate) fn local(job: JobExecution, shutdown: CancellationToken) -> Self {
        Self::Local(LocalTransport {
            pending_job: Arc::new(Mutex::new(Some(job))),
            final_status: Arc::new(Mutex::new(None)),
            shutdown,
        })
    }
}

impl JobClient {
    pub async fn listen_for_job(&self) -> Result<JobExecution, orb_relay_client::Err> {
        if let JobTransport::Local(local) = &self.transport {
            loop {
                let next_job = local.pending_job.lock().await.take();

                if let Some(job) = next_job {
                    info!(
                        job_id = %job.job_id,
                        job_execution_id = %job.job_execution_id,
                        job_document = %redact_job_document(&job.job_document),
                        should_cancel = job.should_cancel,
                        "received local JobExecution"
                    );
                    return Ok(job);
                }

                std::future::pending::<()>().await;
            }
        }

        let relay_client = match &self.transport {
            JobTransport::Relay { relay_client, .. } => relay_client,
            JobTransport::Local(_) => unreachable!(),
        };

        loop {
            match relay_client.recv().await {
                Ok(msg) => {
                    let any = match Any::decode(msg.payload.as_slice()) {
                        Ok(any) => any,
                        Err(e) => {
                            error!("error decoding message: {:?}", e);
                            continue;
                        }
                    };
                    if any.type_url == JobNotify::type_url() {
                        match JobNotify::decode(any.value.as_slice()) {
                            Ok(job_notify) => {
                                info!("received JobNotify: {:?}", job_notify);
                                let _ = self.request_next_job().await;
                            }
                            Err(e) => {
                                error!("error decoding JobNotify: {:?}", e);
                            }
                        }
                    } else if any.type_url == JobExecution::type_url() {
                        match JobExecution::decode(any.value.as_slice()) {
                            Ok(job) => {
                                info!(
                                    job_id = %job.job_id,
                                    job_execution_id = %job.job_execution_id,
                                    job_document = %redact_job_document(&job.job_document),
                                    should_cancel = job.should_cancel,
                                    "received JobExecution"
                                );
                                return Ok(job);
                            }
                            Err(e) => {
                                error!("error decoding JobExecution: {:?}", e);
                            }
                        }
                    } else if any.type_url == JobCancel::type_url() {
                        match JobCancel::decode(any.value.as_slice()) {
                            Ok(job_cancel) => {
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
                            Err(e) => {
                                error!("error decoding JobCancel: {:?}", e);
                            }
                        }
                    } else {
                        error!("received unexpected message type: {:?}", any.type_url);
                    }
                }
                Err(e) => {
                    error!("error receiving from relay: {:?}", e);
                    return Err(e);
                }
            }
        }
    }

    /// Requests for a next job to be run, excluding the ones that are
    /// currently running (determined by `running_job_execution_ids` arg)
    pub async fn request_next_job(&self) -> Result<(), orb_relay_client::Err> {
        let (relay_client, target_service_id, relay_namespace) = match &self.transport {
            JobTransport::Relay {
                relay_client,
                target_service_id,
                relay_namespace,
            } => (relay_client, target_service_id, relay_namespace),
            JobTransport::Local(_) => return Ok(()),
        };

        let mut running_ids = self.job_registry.get_active_job_ids().await;
        let mut completed_ids = self.job_registry.get_completed_job_ids().await;

        running_ids.append(&mut completed_ids);
        let job_ids_to_ignore = running_ids;

        let job_request = JobRequestNext {
            ignore_job_execution_ids: job_ids_to_ignore.clone(),
        };

        let any = Any::from_msg(&job_request).unwrap();
        relay_client
            .send(
                SendMessage::to(EntityType::Service)
                    .id(target_service_id.clone())
                    .namespace(relay_namespace.clone())
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
        if let JobTransport::Local(local) = &self.transport {
            let escaped_stdout =
                serde_json::to_string(&job_update.std_out).unwrap_or_default();
            let escaped_stderr =
                serde_json::to_string(&job_update.std_err).unwrap_or_default();
            let escaped_job_id =
                serde_json::to_string(&job_update.job_id).unwrap_or_default();
            let escaped_execution_id =
                serde_json::to_string(&job_update.job_execution_id).unwrap_or_default();
            let serialized = format!(
                "{{\"job_id\":{escaped_job_id},\"job_execution_id\":{escaped_execution_id},\"status\":{},\"std_out\":{escaped_stdout},\"std_err\":{escaped_stderr}}}",
                job_update.status
            );
            println!("{serialized}");

            if job_update.status
                != orb_relay_messages::jobs::v1::JobExecutionStatus::InProgress as i32
            {
                *local.final_status.lock().await = Some(job_update.status);
                local.shutdown.cancel();
            }

            return Ok(());
        }

        let (relay_client, target_service_id, relay_namespace) = match &self.transport {
            JobTransport::Relay {
                relay_client,
                target_service_id,
                relay_namespace,
            } => (relay_client, target_service_id, relay_namespace),
            JobTransport::Local(_) => unreachable!(),
        };

        info!(
            job_execution_id = %job_update.job_execution_id,
            job_id = %job_update.job_id,
            "sending job update: {:?}",
            job_update
        );
        let any = Any::from_msg(job_update).unwrap();
        relay_client
            .send(
                SendMessage::to(EntityType::Service)
                    .id(target_service_id.clone())
                    .namespace(relay_namespace.clone())
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
        match &self.transport {
            JobTransport::Relay { relay_client, .. } => relay_client
                .reconnect()
                .await
                .map_err(|_| eyre!("failed to force reconnect orb relay")),
            JobTransport::Local(_) => Ok(()),
        }
    }

    pub async fn local_final_status(&self) -> Option<i32> {
        match &self.transport {
            JobTransport::Local(local) => *local.final_status.lock().await,
            JobTransport::Relay { .. } => None,
        }
    }
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
}
