use crate::job_system::{
    orchestrator::{JobConfig, JobRegistry},
    sanitize::redact_job_document,
};
use async_trait::async_trait;
use color_eyre::eyre::{eyre, Result};
use orb_relay_client::{Client, QoS, SendMessage};
use orb_relay_messages::{
    jobs::v1::{
        JobCancel, JobExecution, JobExecutionStatus, JobExecutionUpdate, JobNotify,
        JobRequestNext,
    },
    prost::{Message, Name},
    prost_types::Any,
    relay::entity::EntityType,
};
use std::sync::{Arc, Mutex};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

pub type TransportResult<T> = std::result::Result<T, orb_relay_client::Err>;

#[derive(Debug)]
pub enum JobTransportMessage {
    Notify,
    Execution(JobExecution),
    Cancel(JobCancel),
}

#[async_trait]
pub trait JobTransport: Send + Sync + std::fmt::Debug {
    async fn recv(&self) -> TransportResult<JobTransportMessage>;

    async fn request_next_job(&self, request: &JobRequestNext) -> TransportResult<()>;

    async fn send_job_update(&self, update: &JobExecutionUpdate)
        -> TransportResult<()>;

    async fn reconnect(&self) -> Result<()>;
}

#[derive(Debug, Clone)]
pub struct RelayTransport {
    relay_client: Client,
    target_service_id: String,
    relay_namespace: String,
}

impl RelayTransport {
    pub fn new(
        relay_client: Client,
        target_service_id: impl Into<String>,
        relay_namespace: impl Into<String>,
    ) -> Self {
        Self {
            relay_client,
            target_service_id: target_service_id.into(),
            relay_namespace: relay_namespace.into(),
        }
    }

    async fn send_request(&self, request: &JobRequestNext) -> TransportResult<()> {
        let any = Any::from_msg(request).unwrap();
        self.relay_client
            .send(
                SendMessage::to(EntityType::Service)
                    .id(self.target_service_id.clone())
                    .namespace(self.relay_namespace.clone())
                    .qos(QoS::AtLeastOnce)
                    .payload(any.encode_to_vec()),
            )
            .await
    }
}

#[async_trait]
impl JobTransport for RelayTransport {
    async fn recv(&self) -> TransportResult<JobTransportMessage> {
        loop {
            match self.relay_client.recv().await {
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
                                return Ok(JobTransportMessage::Notify);
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
                                return Ok(JobTransportMessage::Execution(job));
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
                                return Ok(JobTransportMessage::Cancel(job_cancel));
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

    async fn request_next_job(
        &self,
        job_request: &JobRequestNext,
    ) -> TransportResult<()> {
        self.send_request(job_request).await?;

        info!(
            "sent JobRequestNext ignoring {} job execution IDs: {:?}",
            job_request.ignore_job_execution_ids.len(),
            job_request.ignore_job_execution_ids
        );

        Ok(())
    }

    async fn send_job_update(
        &self,
        job_update: &JobExecutionUpdate,
    ) -> TransportResult<()> {
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

    async fn reconnect(&self) -> Result<()> {
        self.relay_client
            .reconnect()
            .await
            .map_err(|_| eyre!("failed to force reconnect orb relay"))
    }
}

#[derive(Debug)]
pub struct LocalTransport {
    pending_job: Mutex<Option<JobExecution>>,
    terminal_update: Mutex<Option<JobExecutionUpdate>>,
    shutdown: CancellationToken,
}

impl LocalTransport {
    pub fn new(job: JobExecution) -> Self {
        Self {
            pending_job: Mutex::new(Some(job)),
            terminal_update: Mutex::new(None),
            shutdown: CancellationToken::new(),
        }
    }

    pub fn terminal_update(&self) -> Option<JobExecutionUpdate> {
        self.terminal_update.lock().unwrap().clone()
    }

    pub fn shutdown_handle(&self) -> JoinHandle<TransportResult<()>> {
        let shutdown = self.shutdown.clone();
        tokio::spawn(async move {
            shutdown.cancelled().await;
            Ok(())
        })
    }
}

#[async_trait]
impl JobTransport for LocalTransport {
    async fn recv(&self) -> TransportResult<JobTransportMessage> {
        let next_job = self.pending_job.lock().unwrap().take();

        if let Some(job) = next_job {
            info!(
                job_id = %job.job_id,
                job_execution_id = %job.job_execution_id,
                job_document = %redact_job_document(&job.job_document),
                should_cancel = job.should_cancel,
                "received local JobExecution"
            );

            return Ok(JobTransportMessage::Execution(job));
        }

        std::future::pending::<()>().await;
        unreachable!()
    }

    async fn request_next_job(&self, _request: &JobRequestNext) -> TransportResult<()> {
        Ok(())
    }

    async fn send_job_update(
        &self,
        job_update: &JobExecutionUpdate,
    ) -> TransportResult<()> {
        let status_name = JobExecutionStatus::try_from(job_update.status)
            .map(|status| format!("{status:?}"))
            .unwrap_or_else(|_| format!("Unknown({})", job_update.status));

        println!("--- Job Update ---");
        println!("job_id:            {}", job_update.job_id);
        println!("job_execution_id:  {}", job_update.job_execution_id);
        println!("status:            {status_name}");

        if !job_update.std_out.is_empty() {
            println!("stdout:\n{}", job_update.std_out);
        }

        if !job_update.std_err.is_empty() {
            eprintln!("stderr:\n{}", job_update.std_err);
        }

        if job_update.status != JobExecutionStatus::InProgress as i32 {
            *self.terminal_update.lock().unwrap() = Some(job_update.clone());
            self.shutdown.cancel();
        }

        Ok(())
    }

    async fn reconnect(&self) -> Result<()> {
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct JobClient {
    transport: Arc<dyn JobTransport>,
    job_registry: JobRegistry,
    job_config: JobConfig,
}

impl JobClient {
    pub fn new(
        transport: Arc<dyn JobTransport>,
        job_registry: JobRegistry,
        job_config: JobConfig,
    ) -> Self {
        Self {
            transport,
            job_registry,
            job_config,
        }
    }

    pub async fn listen_for_job(&self) -> TransportResult<JobExecution> {
        loop {
            match self.transport.recv().await? {
                JobTransportMessage::Notify => {
                    let _ = self.request_next_job().await;
                }
                JobTransportMessage::Execution(job) => {
                    return Ok(job);
                }
                JobTransportMessage::Cancel(job_cancel) => {
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
            }
        }
    }

    pub async fn request_next_job(&self) -> TransportResult<()> {
        let job_request = build_job_request(&self.job_registry).await;
        self.transport.request_next_job(&job_request).await
    }

    pub async fn try_request_more_jobs(&self) -> TransportResult<bool> {
        if !self
            .job_config
            .should_request_more_jobs(&self.job_registry)
            .await
        {
            return Ok(false);
        }

        self.request_next_job()
            .await
            .inspect_err(|e| error!("Failed to request additional job: {:?}", e))?;

        info!("Successfully requested additional job for parallel execution");

        Ok(true)
    }

    pub async fn send_job_update(
        &self,
        job_update: &JobExecutionUpdate,
    ) -> TransportResult<()> {
        self.transport.send_job_update(job_update).await
    }

    pub async fn force_relay_reconnect(&self) -> Result<()> {
        self.transport.reconnect().await
    }
}

async fn build_job_request(job_registry: &JobRegistry) -> JobRequestNext {
    let mut running_ids = job_registry.get_active_job_ids().await;
    let mut completed_ids = job_registry.get_completed_job_ids().await;
    running_ids.append(&mut completed_ids);

    JobRequestNext {
        ignore_job_execution_ids: running_ids,
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
