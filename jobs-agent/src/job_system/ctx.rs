use crate::program::Deps;

use super::{client::JobClient, handler::Handler};
use bon::bon;
use orb_relay_messages::jobs::v1::{
    JobExecution, JobExecutionStatus, JobExecutionUpdate,
};
use std::{collections::HashMap, sync::Arc};
use tokio_util::sync::CancellationToken;
use tracing::error;

#[derive(Debug, Clone)]
pub struct Ctx {
    job: JobExecution,
    job_args: Vec<String>,
    job_client: JobClient,
    cancel_token: CancellationToken,
    deps: Arc<Deps>
}

#[bon]
impl Ctx {
    pub async fn try_build(
        deps: Arc<Deps>,
        handlers: &mut HashMap<String, Handler>,
        job: JobExecution,
        job_client: JobClient,
        cancel_token: CancellationToken,
    ) -> Option<(Ctx, Handler)> {
        let mut ctx = Ctx {
            deps,
            job,
            job_args: vec![], // todo: fill out
            job_client,
            cancel_token,
        };

        // system is made to expect commands to be
        // whitespace separate words, where the last part of the command
        // might be args.
        // e.g.: systemctl restart orb-core
        // if "systemctl restart" is registered as a command, orb-core will be the resulting argument
        let key_handler_pair = handlers
            .iter()
            .find(|(k, _)| ctx.job.job_document.starts_with(*k));

        let (command, handler) = match key_handler_pair.map(|(c, h)| (c, h.clone())) {
            None => {
                let update = ctx.status(JobExecutionStatus::FailedUnsupported);

                if let Err(e) = ctx.job_client.send_job_update(&update).await {
                    error!(
                        "failed to send job updated for job FailedUnsupported job: '{}'. Err: {:?}",
                        ctx.job.job_document, e
                    );
                }

                return None;
            }

            Some((c, h)) => (c, h),
        };

        let args: Vec<_> = ctx
            .job
            .job_document
            .replace(command, "")
            .trim()
            .split(" ")
            .map(String::from)
            .collect();

        ctx.job_args.extend(args);

        Some((ctx, handler))
    }

    pub fn execution_id(&self) -> &str {
        self.job.job_execution_id.as_str()
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel_token.is_cancelled()
    }

    pub fn cancel(&self) {
        self.cancel_token.cancel()
    }

    pub fn deps(&self) -> &Arc<Deps> {
       &self.deps
    }

    // TODO: doccomment with example
    pub fn status(&self, status: JobExecutionStatus) -> JobExecutionUpdate {
        JobExecutionUpdate {
            job_id: self.job.job_id.clone(),
            job_execution_id: self.job.job_execution_id.clone(),
            status: status as i32,
            std_out: String::new(),
            std_err: String::new(),
        }
    }

    // TODO: doccomment with example
    pub fn success(&self) -> JobExecutionUpdate {
        self.status(JobExecutionStatus::Succeeded)
    }

    // TODO: doccomment with example
    pub fn failure(&self) -> JobExecutionUpdate {
        self.status(JobExecutionStatus::Failed)
    }

    #[builder(finish_fn = send)]
    #[builder(on(String, into))]
    pub async fn progress(
        &self,
        #[builder(default = "".to_string())] stdout: String,
        #[builder(default = "".to_string())] stderr: String,
    ) -> Result<(), orb_relay_client::Err> {
        let mut update = self.status(JobExecutionStatus::InProgress);
        update.std_out = stdout;
        update.std_err = stderr;
        self.job_client.send_job_update(&update).await
    }

    pub fn args(&self) -> &Vec<String> {
        &self.job_args
    }
}

pub trait JobExecutionUpdateExt: Sized {
    fn status(self, status: JobExecutionStatus) -> Self;
    fn stdout(self, std_out: impl Into<String>) -> Self;
    fn stderr(self, std_err: impl Into<String>) -> Self;
}

impl JobExecutionUpdateExt for JobExecutionUpdate {
    fn status(mut self, status: JobExecutionStatus) -> Self {
        self.status = status as i32;
        self
    }

    fn stdout(mut self, std_out: impl Into<String>) -> Self {
        self.std_out = std_out.into();
        self
    }

    fn stderr(mut self, std_err: impl Into<String>) -> Self {
        self.std_err = std_err.into();
        self
    }
}
