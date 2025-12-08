use super::ctx::Ctx;
use crate::{
    job_system::{
        client::JobClient,
        ctx::JobExecutionUpdateExt,
        orchestrator::{JobCompletion, JobConfig, JobRegistry, JobStartStatus},
        sanitize::redact_job_document,
    },
    program::Deps,
    settings::Settings,
};
use color_eyre::Result;
use orb_relay_client::{Client, ClientOpts};
use orb_relay_messages::{
    jobs::v1::{JobExecution, JobExecutionStatus, JobExecutionUpdate},
    relay::entity::EntityType,
};
use std::{collections::HashMap, pin::Pin, sync::Arc, time::Duration};
use tokio::{sync::oneshot, task::JoinHandle};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

pub type Handler = Arc<
    dyn Fn(
            Ctx,
        )
            -> Pin<Box<dyn Future<Output = Result<JobExecutionUpdate>> + Send + 'static>>
        + Send
        + Sync,
>;

pub struct JobHandlerBuilder {
    job_config: JobConfig,
    handlers: HashMap<String, Handler>,
}

impl JobHandlerBuilder {
    pub fn parallel<H, Fut>(mut self, on_cmd: &str, handler: H) -> Self
    where
        H: Fn(Ctx) -> Fut + 'static + Send + Sync,
        Fut: Future<Output = Result<JobExecutionUpdate>> + 'static + Send,
    {
        self.handlers
            .insert(on_cmd.into(), Arc::new(move |ctx| Box::pin(handler(ctx))));

        self.job_config.parallel_job(on_cmd, None);

        self
    }

    pub fn parallel_max<H, Fut>(mut self, on_cmd: &str, max: usize, handler: H) -> Self
    where
        H: Fn(Ctx) -> Fut + 'static + Send + Sync,
        Fut: Future<Output = Result<JobExecutionUpdate>> + 'static + Send,
    {
        self.handlers
            .insert(on_cmd.into(), Arc::new(move |ctx| Box::pin(handler(ctx))));

        self.job_config.parallel_job(on_cmd, Some(max));

        self
    }

    pub fn sequential<H, Fut>(mut self, on_cmd: &str, handler: H) -> Self
    where
        H: Fn(Ctx) -> Fut + 'static + Send + Sync,
        Fut: Future<Output = Result<JobExecutionUpdate>> + 'static + Send,
    {
        self.handlers
            .insert(on_cmd.into(), Arc::new(move |ctx| Box::pin(handler(ctx))));

        self.job_config.sequential_job(on_cmd);

        self
    }

    pub fn build(self, deps: Deps) -> JobHandler {
        JobHandler::new(self, deps)
    }
}

/// Routes commands to their handlers, spawning them in a new `tokio::task`.
///
/// Commands are expected to be a sequence of whitespace separated
/// words followed by arguments.
///
/// e.g.:
/// ```ignore
/// JobHandler::builder()
///     .parallel("read_file", read_file::handler)
///     .parallel("mcu", mcu::handler)
///     .parallel_max("logs", 3, logs::handler)
///     .build(deps)
///     .run()
///     .await;
/// ```
///
/// In the above setup, if we received the command `read_file /home/worldcoin/bla.txt`,
/// `read_file` would be the command, while the received args in the handler would be
/// `["/home/worldcoin/bla.txt"]`.
///
/// If we received the command `mcu main reboot`, `mcu` would be the command, and the args
/// would be `["main", "reboot"]`
pub struct JobHandler {
    state: Arc<Deps>,
    job_config: JobConfig,
    job_registry: JobRegistry,
    job_client: JobClient,
    relay_handle: JoinHandle<Result<(), orb_relay_client::Err>>,
    handlers: HashMap<String, Handler>,
}

impl JobHandler {
    pub fn builder() -> JobHandlerBuilder {
        JobHandlerBuilder {
            job_config: JobConfig::new(),
            handlers: HashMap::new(),
        }
    }

    fn new(builder: JobHandlerBuilder, deps: Deps) -> Self {
        let Settings {
            orb_id,
            relay_host,
            relay_namespace,
            target_service_id,
            auth,
            ..
        } = &deps.settings;

        let opts = ClientOpts::entity(EntityType::Orb)
            .id(orb_id.as_str().to_string())
            .endpoint(relay_host)
            .namespace(relay_namespace)
            .auth(auth.clone())
            .connection_timeout(Duration::from_secs(3))
            .connection_backoff(Duration::from_secs(2))
            .keep_alive_interval(Duration::from_secs(30))
            .keep_alive_timeout(Duration::from_secs(10))
            .ack_timeout(Duration::from_secs(5))
            .build();

        info!("Connecting to relay: {:?}", relay_host);
        let (relay_client, relay_handle) = Client::connect(opts);
        let job_registry = JobRegistry::new();
        let job_config = builder.job_config;
        let job_client = JobClient::new(
            relay_client.clone(),
            target_service_id.as_str(),
            relay_namespace,
            job_registry.clone(),
            job_config.clone(),
        );

        Self {
            state: Arc::new(deps),
            job_config,
            job_registry,
            job_client,
            relay_handle,
            handlers: builder.handlers.into_iter().collect(),
        }
    }

    pub async fn run(mut self) {
        // Kickstart job requests.
        match self.job_client.try_request_more_jobs().await {
            Ok(true) => {
                info!("[JOB_EXECUTION] Successfully requested initial job");
            }

            Ok(false) => {
                // No jobs available, try basic request
                if let Err(e) = self.job_client.request_next_job().await {
                    error!("[JOB_EXECUTION] Failed to request initial job: {:?}", e);
                }
            }

            Err(e) => {
                error!("[JOB_EXECUTION] Failed to request initial job via parallel logic: {:?}, trying basic request", e);
                if let Err(e) = self.job_client.request_next_job().await {
                    error!("[JOB_EXECUTION] Failed to request initial job: {:?}", e);
                }
            }
        };

        loop {
            tokio::select! {
                _ = &mut self.relay_handle => {
                    info!("Relay service shutdown detected");
                    break;
                }

                Ok(job) = self.job_client.listen_for_job() => {
                    self = self.handle_job(job).await;
                }
            }
        }
    }

    async fn handle_job(mut self, job: JobExecution) -> Self {
        info!("[JOB_EXECUTION] Processing job: {:?}", job.job_id);

        // Check if job is already cancelled
        if job.should_cancel {
            info!("[JOB_EXECUTION] Job {} is already marked for cancellation, acknowledging and skipping execution", job.job_execution_id);

            // Send cancellation acknowledgment
            let cancel_update = JobExecutionUpdate {
                job_id: job.job_id.clone(),
                job_execution_id: job.job_execution_id.clone(),
                status: JobExecutionStatus::Cancelled as i32,
                std_out: String::new(),
                std_err: String::new(),
            };

            if let Err(e) = self.job_client.send_job_update(&cancel_update).await {
                error!(
                    "[JOB_EXECUTION] Failed to send cancellation acknowledgment: {:?}",
                    e
                );
            }

            // Request next job immediately after cancellation acknowledgment
            match self.job_client.try_request_more_jobs().await {
                Ok(true) => {
                    info!(
                        "[JOB_EXECUTION] Successfully requested job after cancellation acknowledgment"
                    );
                }
                Ok(false) => {
                    // No more jobs or at limits, try basic request
                    if let Err(e) = self.job_client.request_next_job().await {
                        error!("[JOB_EXECUTION] Failed to request next job after cancellation acknowledgment: {:?}", e);
                    }
                }
                Err(e) => {
                    error!("[JOB_EXECUTION] Failed to request job via parallel logic after cancellation: {:?}, trying basic request", e);
                    if let Err(e) = self.job_client.request_next_job().await {
                        error!("[JOB_EXECUTION] Failed to request next job after cancellation acknowledgment: {:?}", e);
                    }
                }
            }

            return self;
        }

        let cancel_token = CancellationToken::new();

        let (ctx, handler) = match Ctx::try_build(
            self.state.clone(),
            &mut self.handlers,
            job.clone(),
            self.job_client.clone(),
            cancel_token.clone(),
        )
        .await
        {
            None => return self,
            Some((a, b)) => (a, b),
        };

        // Check if this job can be started based on parallelization rules
        let job_type = job.job_document.clone();
        match self
            .job_config
            .can_start_job(ctx.cmd(), &self.job_registry)
            .await
        {
            JobStartStatus::Allowed => (),
            JobStartStatus::UnknownJob => {
                warn!(
                    "[JOB_EXECUTION] Job '{}' of type '{}' is unknown, will not start",
                    job.job_execution_id, job_type
                );

                if let Err(e) = self
                    .job_client
                    .send_job_update(&JobExecutionUpdate {
                        job_id: job.job_id.clone(),
                        job_execution_id: job.job_execution_id.clone(),
                        status: JobExecutionStatus::FailedUnsupported as i32,
                        std_out: String::new(),
                        std_err: String::new(),
                    })
                    .await
                {
                    error!(
                        "[JOB_EXECUTION] failed to send job update for unsupported job {job_type}, err: {e:?}",
                    );
                }

                // Send a message indicating we're skipping this job and request another
                match self.job_client.try_request_more_jobs().await {
                    Ok(true) => {
                        info!(
                            "[JOB_EXECUTION] Requested alternative job after skipping incompatible job"
                        );
                    }
                    Ok(false) => {
                        if let Err(e) = self.job_client.request_next_job().await {
                            error!(
                                "[JOB_EXECUTION] Failed to request next job after skipping: {:?}",
                                e
                            );
                        }
                    }
                    Err(e) => {
                        error!("[JOB_EXECUTION] Failed to request job after skipping: {:?}", e);
                    }
                }
            }

            cannot_start_status => {
                info!("[JOB_EXECUTION] Job '{}' of type '{}' cannot be started due to {cannot_start_status:?}",
                  job.job_execution_id, job_type);

                return self;
            }
        }

        if self.job_registry.is_job_active(ctx.execution_id()).await {
            info!(
                "[JOB_EXECUTION] trying to execute a job that is already running. ignored. {} {}",
                ctx.cmd(),
                ctx.execution_id()
            );

            return self;
        }

        // Create completion channel for this job
        let (completion_tx, completion_rx) = oneshot::channel();

        // Register job for cancellation tracking
        let job_handle = tokio::spawn(async move {
            // This is a placeholder for the actual job execution
            // The real implementation would be more complex
        });

        self.job_registry
            .register_job(
                job.job_execution_id.clone(),
                job.job_document.clone(),
                cancel_token.clone(),
                job_handle,
            )
            .await;

        let job_client = self.job_client.clone();
        let job_clone = job.clone();
        let update = ctx.status(JobExecutionStatus::InProgress);
        let job_registry = self.job_registry.clone();

        tokio::spawn(async move {
            if ctx.is_cancelled() {
                let update = ctx.failure().stdout("Job was cancelled");

                if let Err(e) = job_client.send_job_update(&update).await {
                    error!("[JOB_EXECUTION] Failed to send job update: {:?}", e);
                }

                let _ = completion_tx
                    .send(JobCompletion::cancelled(ctx.execution_id().to_owned()));

                return;
            }

            if job_registry.is_job_completed(ctx.execution_id()).await {
                info!(
                    "[JOB_EXECUTION] trying to execute a job that was recently completed. ignored. {} {}",
                    ctx.cmd(),
                    ctx.execution_id()
                );

                return;
            }

            info!(
                execution_id = ctx.execution_id(),
                command = ctx.cmd(),
                args = ctx.args_raw(),
                "[JOB_EXECUTION] executing {}",
                ctx.cmd()
            );

            match handler(ctx).await {
                Err(e) => {
                    let e = e.to_string();
                    error!(
                        job_execution_id = %job_clone.job_execution_id,
                        job_document = %redact_job_document(&job_clone.job_document),
                        error = %e,
                        "[JOB_EXECUTION] handler failed"
                    );

                    let update =
                        update.status(JobExecutionStatus::Failed).stderr(e.clone());

                    if let Err(e) = job_client.send_job_update(&update).await {
                        error!(
                            "[JOB_EXECUTION] failed to send failed job update for job {}. Err: {e:?}",
                            job_clone.job_id
                        )
                    }

                    let completion =
                        JobCompletion::failure(job_clone.job_execution_id.clone(), e);

                    if let Err(e) = completion_tx.send(completion) {
                        error!(
                            "[JOB_EXECUTION] failed to send job completion for job {}. Err: {e:?}",
                            job_clone.job_id
                        )
                    }
                }

                Ok(update) => {
                    if let Err(e) = job_client.send_job_update(&update).await {
                        error!(
                            "[JOB_EXECUTION] failed to send failed job update for job {}. Err: {e:?}",
                            job_clone.job_id
                        )
                    }

                    let completion =
                        JobCompletion::success(job_clone.job_execution_id.clone());

                    if let Err(e) = completion_tx.send(completion) {
                        error!(
                            "[JOB_EXECUTION] failed to send job completion for job {}. Err: {e:?}",
                            job_clone.job_id
                        )
                    }
                }
            }
        });

        // Check if this job supports parallel execution and request more jobs if appropriate
        if self.job_config.is_parallel(&job_type) {
            info!(
                "[JOB_EXECUTION] Started parallel job '{}', checking for additional jobs",
                job_type
            );

            // Try to request more jobs for parallel execution
            match self.job_client.try_request_more_jobs().await {
                Ok(true) => {
                    info!(
                        "[JOB_EXECUTION] Successfully requested additional job for parallel execution"
                    );
                }
                Ok(false) => {
                    info!("[JOB_EXECUTION] No additional jobs requested (at parallelization limits or no jobs available)");
                }
                Err(e) => {
                    error!(
                        "[JOB_EXECUTION] Failed to request additional job for parallel execution: {:?}",
                        e
                    );
                }
            }
        } else if self.job_config.is_sequential(&job_type) {
            info!(
                "[JOB_EXECUTION] Started sequential job '{}', will not request additional jobs",
                job_type
            );
        }

        let job_registry_clone = self.job_registry.clone();

        // Wait for job completion in a separate task
        let job_client_for_completion = self.job_client.clone();
        let job_execution_id = job.job_execution_id.clone();
        tokio::spawn(async move {
            match completion_rx.await {
                Ok(completion) => {
                    info!(
                        "[JOB_EXECUTION] Job {} completed with status: {:?}",
                        job_execution_id, completion.status
                    );

                    // Unregister job
                    job_registry_clone.unregister_job(&job_execution_id).await;

                    if completion.status == JobExecutionStatus::InProgress {
                        info!("[JOB_EXECUTION] Job completed with InProgress status. Will not request a new job.");
                        return;
                    }

                    // Try to request more jobs for parallel execution
                    match job_client_for_completion.try_request_more_jobs().await {
                        Ok(true) => {
                            info!("[JOB_EXECUTION] Requested additional job after job completion");
                        }
                        Ok(false) => {
                            // No more jobs available or at limits, just request next job normally
                            if let Err(e) =
                                job_client_for_completion.request_next_job().await
                            {
                                error!(
                                    "[JOB_EXECUTION] Failed to request next job: {:?}",
                                    e
                                );
                            }
                        }
                        Err(e) => {
                            error!("[JOB_EXECUTION] Failed to request additional job: {:?}, trying normal request", e);
                            // Fallback to normal job request
                            if let Err(e) =
                                job_client_for_completion.request_next_job().await
                            {
                                error!(
                                    "[JOB_EXECUTION] Failed to request next job: {:?}",
                                    e
                                );
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("[JOB_EXECUTION] Job completion channel error: {:?}", e);

                    // Unregister job
                    job_registry_clone.unregister_job(&job_execution_id).await;

                    // Still try to request more jobs
                    match job_client_for_completion.try_request_more_jobs().await {
                        Ok(_) => {}
                        Err(e) => {
                            error!("[JOB_EXECUTION] Failed to request additional job after error: {:?}, trying normal request", e);
                            if let Err(e) =
                                job_client_for_completion.request_next_job().await
                            {
                                error!(
                                    "[JOB_EXECUTION] Failed to request next job: {:?}",
                                    e
                                );
                            }
                        }
                    }
                }
            }
        });

        self
    }
}
