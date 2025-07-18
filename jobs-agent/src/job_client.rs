use color_eyre::eyre::Result;
use orb_relay_client::{Client, SendMessage};
use orb_relay_messages::{
    jobs::v1::{JobCancel, JobExecution, JobExecutionUpdate, JobNotify, JobRequestNext},
    prost::{Message, Name},
    prost_types::Any,
    relay::entity::EntityType,
};
use tracing::{error, info, warn};

use crate::orchestrator::{JobConfig, JobRegistry};

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
                                let _ = self.request_next_job().await;
                            }
                            Err(e) => {
                                error!("error decoding JobNotify: {:?}", e);
                            }
                        }
                    } else if any.type_url == JobExecution::type_url() {
                        match JobExecution::decode(any.value.as_slice()) {
                            Ok(job) => {
                                info!("received JobExecution: {:?}", job);
                                return Ok(job);
                            }
                            Err(e) => {
                                error!("error decoding JobExecution: {:?}", e);
                            }
                        }
                    } else if any.type_url == JobCancel::type_url() {
                        match JobCancel::decode(any.value.as_slice()) {
                            Ok(job_cancel) => {
                                info!("received JobCancel: {:?}", job_cancel);
                                let cancelled = self.job_registry.cancel_job(&job_cancel.job_execution_id).await;
                                if cancelled {
                                    info!("Successfully cancelled job: {}", job_cancel.job_execution_id);
                                } else {
                                    warn!("Attempted to cancel non-existent or already completed job: {}", job_cancel.job_execution_id);
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

    pub async fn request_next_job(&self) -> Result<(), orb_relay_client::Err> {
        self.request_next_job_with_running_ids(&[]).await
    }

    pub async fn request_next_job_with_running_ids(&self, running_job_execution_ids: &[String]) -> Result<(), orb_relay_client::Err> {
        // Create JobRequestNext with ignore_job_execution_ids field
        let job_request = if running_job_execution_ids.is_empty() {
            JobRequestNext::default()
        } else {
            // Create JobRequestNext with running job execution IDs to ignore
            self.create_job_request_with_ignore_ids(running_job_execution_ids)
        };

        let any = Any::from_msg(&job_request).unwrap();
        match self
            .relay_client
            .send(
                SendMessage::to(EntityType::Service)
                    .id(self.target_service_id.clone())
                    .namespace(self.relay_namespace.clone())
                    .payload(any.encode_to_vec()),
            )
            .await
        {
            Ok(_) => {
                if running_job_execution_ids.is_empty() {
                    info!("sent JobRequestNext");
                } else {
                    info!("sent JobRequestNext ignoring {} job execution IDs: {:?}", 
                          running_job_execution_ids.len(), running_job_execution_ids);
                }
                Ok(())
            }
            Err(e) => {
                error!("error sending JobRequestNext: {:?}", e);
                Err(e)
            }
        }
    }

    fn create_job_request_with_ignore_ids(&self, running_job_execution_ids: &[String]) -> JobRequestNext {
        // Create a JobRequestNext with ignore_job_execution_ids field populated
        // This tells the backend to ignore these job execution IDs when determining the next job
        JobRequestNext {
            ignore_job_execution_ids: running_job_execution_ids.to_vec(),
        }
    }

    /// Check if we should request more jobs and do so if appropriate
    /// This method is used to implement parallel job execution
    pub async fn try_request_more_jobs(&self) -> Result<bool, orb_relay_client::Err> {
        // Check if we should request more jobs based on current configuration
        if !self.job_config.should_request_more_jobs(&self.job_registry).await {
            return Ok(false);
        }

        // Get currently running job execution IDs
        let running_job_ids = self.job_registry.get_active_job_ids().await;
        
        // Request next job with current running job IDs
        match self.request_next_job_with_running_ids(&running_job_ids).await {
            Ok(()) => {
                info!("Successfully requested additional job for parallel execution");
                Ok(true)
            }
            Err(e) => {
                error!("Failed to request additional job: {:?}", e);
                Err(e)
            }
        }
    }

    pub async fn send_job_update(
        &self,
        job_update: &JobExecutionUpdate,
    ) -> Result<(), orb_relay_client::Err> {
        info!("sending job update: {:?}", job_update);
        let any = Any::from_msg(job_update).unwrap();
        match self
            .relay_client
            .send(
                SendMessage::to(EntityType::Service)
                    .id(self.target_service_id.clone())
                    .namespace(self.relay_namespace.clone())
                    .payload(any.encode_to_vec()),
            )
            .await
        {
            Ok(_) => {
                info!("sent JobExecutionUpdate");
                Ok(())
            }
            Err(e) => {
                error!("error sending JobExecutionUpdate: {:?}", e);
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orb_relay_messages::jobs::v1::{JobExecution, JobExecutionStatus, JobExecutionUpdate};

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

        assert!(!normal_job.should_cancel, "Normal job should not be cancelled");
        assert!(cancelled_job.should_cancel, "Cancelled job should be marked as cancelled");
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
