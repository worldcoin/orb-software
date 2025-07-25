use orb_relay_messages::jobs::v1::JobExecutionStatus;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

/// JobRegistry tracks active jobs and provides cancellation support
///
/// This is the core component that enables:
/// - Job cancellation by execution ID
/// - Parallel job execution tracking
/// - Proper cleanup of completed jobs
#[derive(Debug, Clone)]
pub struct JobRegistry {
    active_jobs: Arc<Mutex<HashMap<String, ActiveJob>>>,
}

#[derive(Debug)]
struct ActiveJob {
    job_type: String,
    cancel_token: CancellationToken,
    handle: tokio::task::JoinHandle<()>,
}

impl Default for JobRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl JobRegistry {
    pub fn new() -> Self {
        Self {
            active_jobs: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Register a new active job with cancellation support
    pub async fn register_job(
        &self,
        job_execution_id: String,
        job_type: String,
        cancellation_token: CancellationToken,
        task_handle: JoinHandle<()>,
    ) {
        let mut jobs = self.active_jobs.lock().await;
        jobs.insert(
            job_execution_id.clone(),
            ActiveJob {
                job_type,
                cancel_token: cancellation_token,
                handle: task_handle,
            },
        );
    }

    /// Unregister a completed job
    pub async fn unregister_job(&self, job_execution_id: &str) -> bool {
        let mut jobs = self.active_jobs.lock().await;
        jobs.remove(job_execution_id).is_some()
    }

    /// Cancel a job by execution ID
    pub async fn cancel_job(&self, job_execution_id: &str) -> bool {
        let jobs = self.active_jobs.lock().await;
        if let Some(active_job) = jobs.get(job_execution_id) {
            info!("Cancelling job: {}", job_execution_id);
            active_job.cancel_token.cancel();
            active_job.handle.abort();
            true
        } else {
            warn!("Attempted to cancel non-existent job: {}", job_execution_id);
            false
        }
    }

    /// Get count of active jobs by type (for parallelization decisions)
    pub async fn get_active_job_count_by_type(&self, job_type: &str) -> usize {
        let jobs = self.active_jobs.lock().await;
        jobs.values().filter(|job| job.job_type == job_type).count()
    }

    /// Get all active job execution IDs
    pub async fn get_active_job_ids(&self) -> Vec<String> {
        let jobs = self.active_jobs.lock().await;
        jobs.keys().cloned().collect()
    }

    /// Check if a specific job is still active
    pub async fn is_job_active(&self, job_execution_id: &str) -> bool {
        let jobs = self.active_jobs.lock().await;
        jobs.contains_key(job_execution_id)
    }
}

/// JobConfig defines parallelization rules for different job types
///
/// This configuration determines:
/// - Which jobs can run in parallel
/// - Which jobs must run sequentially
/// - Maximum concurrent jobs per type
#[derive(Debug, Clone)]
pub struct JobConfig {
    /// Job types that can run in parallel with others
    parallel_jobs: HashSet<String>,
    /// Job types that must run sequentially (block other jobs)
    sequential_jobs: HashSet<String>,
    /// Maximum concurrent jobs per type (None = unlimited)
    max_concurrent_per_type: HashMap<String, usize>,
}

impl Default for JobConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl JobConfig {
    pub fn new() -> Self {
        Self {
            parallel_jobs: HashSet::new(),
            sequential_jobs: HashSet::new(),
            max_concurrent_per_type: HashMap::new(),
        }
    }

    pub fn sequential_job(&mut self, job: &str) {
        self.sequential_jobs.insert(job.to_string());
    }

    pub fn parallel_job(&mut self, job: &str, max_concurrent: Option<usize>) {
        self.parallel_jobs.insert(job.to_string());
        if let Some(max_concurrent) = max_concurrent {
            self.max_concurrent_per_type
                .insert(job.to_string(), max_concurrent);
        }
    }

    /// Check if a job of the given type can be started based on concurrency limits
    pub async fn can_start_job(&self, job_type: &str, registry: &JobRegistry) -> bool {
        // Check if this is a sequential job
        if self.sequential_jobs.contains(job_type) {
            // Sequential jobs can only run if no other jobs are currently active
            let active_jobs = registry.get_active_job_ids().await;
            if !active_jobs.is_empty() {
                return false;
            }
            return true;
        }

        // Check if any sequential jobs are currently running
        // If so, no parallel jobs can start
        let active_jobs = registry.active_jobs.lock().await;
        for job in active_jobs.values() {
            if self.sequential_jobs.contains(&job.job_type) {
                return false;
            }
        }
        drop(active_jobs);

        // Check concurrent limits for parallel jobs
        if self.parallel_jobs.contains(job_type) {
            if let Some(&max_concurrent) = self.max_concurrent_per_type.get(job_type) {
                let current_count =
                    registry.get_active_job_count_by_type(job_type).await;
                return current_count < max_concurrent;
            }
            // No specific limit for this parallel job type, allow it
            return true;
        }

        // Unknown job type, default to allow (with warning)
        tracing::warn!("Unknown job type '{}', allowing execution", job_type);
        true
    }

    pub fn is_sequential(&self, job_type: &str) -> bool {
        self.sequential_jobs.contains(job_type)
    }

    pub fn is_parallel(&self, job_type: &str) -> bool {
        self.parallel_jobs.contains(job_type)
    }

    pub fn get_max_concurrent(&self, job_type: &str) -> Option<usize> {
        self.max_concurrent_per_type.get(job_type).copied()
    }

    /// Check if we should request more jobs based on current active jobs
    /// Returns true if we can potentially start more parallel jobs
    pub async fn should_request_more_jobs(&self, registry: &JobRegistry) -> bool {
        // Check if any sequential jobs are running
        let active_jobs = registry.active_jobs.lock().await;
        for job in active_jobs.values() {
            if self.sequential_jobs.contains(&job.job_type) {
                // Sequential job is running, don't request more
                return false;
            }
        }
        drop(active_jobs);

        // Check if we have room for more parallel jobs
        // For now, if we have any parallel job types that could accept more jobs, request more
        for job_type in &self.parallel_jobs {
            if let Some(&max_concurrent) = self.max_concurrent_per_type.get(job_type) {
                let current_count =
                    registry.get_active_job_count_by_type(job_type).await;
                if current_count < max_concurrent {
                    return true;
                }
            } else {
                // No limit for this job type, we can always request more
                return true;
            }
        }

        // All parallel job types are at their limits, don't request more
        false
    }
}

/// JobCompletion signals when a job finishes
/// This allows handlers to signal completion and provide final status
#[derive(Debug)]
pub struct JobCompletion {
    pub job_execution_id: String,
    pub status: JobExecutionStatus,
    pub final_message: Option<String>,
}

impl JobCompletion {
    pub fn success(job_execution_id: String) -> Self {
        Self {
            job_execution_id,
            status: JobExecutionStatus::Succeeded,
            final_message: None,
        }
    }

    pub fn failure(job_execution_id: String, error: String) -> Self {
        Self {
            job_execution_id,
            status: JobExecutionStatus::Failed,
            final_message: Some(error),
        }
    }

    pub fn cancelled(job_execution_id: String) -> Self {
        Self {
            job_execution_id,
            status: JobExecutionStatus::Failed,
            final_message: Some("Job was cancelled".to_string()),
        }
    }
}

/// Future extension point for job cancellation messages
/// This would be used when the relay sends job cancellation requests
#[derive(Debug)]
pub struct JobCancellationRequest {
    pub job_execution_id: String,
    pub reason: Option<String>,
}
