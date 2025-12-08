use super::{client::JobClient, handler::Handler, sanitize::redact_job_document};
use crate::program::Deps;
use bon::bon;
use color_eyre::eyre::ContextCompat;
use orb_relay_messages::jobs::v1::{
    JobExecution, JobExecutionStatus, JobExecutionUpdate,
};
use serde::Deserialize;
use std::{collections::HashMap, sync::Arc};
use tokio_util::sync::CancellationToken;
use tracing::error;

/// A struct created every time one of the job handlers are called.
/// Contains:
/// - helpers to build `JobExecutionUpdate` needed on handler response
/// - helpers to send progress reports while handler is not done
#[derive(Clone)]
pub struct Ctx {
    cmd: String,
    job: JobExecution,
    job_args: Option<String>,
    job_client: JobClient,
    cancel_token: CancellationToken,
    deps: Arc<Deps>,
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
            cmd: String::new(),
            deps,
            job,
            job_args: None,
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
                        job_document = %redact_job_document(&ctx.job.job_document),
                        error = ?e,
                        "failed to send job update for FailedUnsupported job"
                    );
                }

                return None;
            }

            Some((c, h)) => (c, h),
        };

        ctx.job_args = ctx
            .job
            .job_document
            .split_once(command)
            .map(|(_cmd, args_raw)| args_raw.trim())
            .filter(|args_raw| !args_raw.is_empty())
            .map(String::from);

        ctx.cmd.push_str(command);

        Some((ctx, handler))
    }

    pub fn cmd(&self) -> &str {
        self.cmd.as_str()
    }

    /// Returns the `job_execution_id` of the current job.
    pub fn execution_id(&self) -> &str {
        self.job.job_execution_id.as_str()
    }

    // Returns `true` if current job has been cancelled.
    // This is typically already checked before the handler is called, so unless
    // the handler has a long running task there is no need to call this.
    pub fn is_cancelled(&self) -> bool {
        self.cancel_token.is_cancelled()
    }

    /// Returns a reference to the dependencies registered
    /// in `program.rs`.
    pub fn deps(&self) -> &Arc<Deps> {
        &self.deps
    }

    /// Helper method to create a `JobExecutionUpdate` with the appropriate
    /// `job_id` and `job_execution_id`.
    /// ```ignore
    /// pub async fn handler(ctx: Ctx) -> Result<JobExecutionUpdate> {
    ///    println!("i ran!");
    ///    Ok(ctx.status(JobExecutionStatus::Succceeded))
    /// }
    /// ```
    pub fn status(&self, status: JobExecutionStatus) -> JobExecutionUpdate {
        JobExecutionUpdate {
            job_id: self.job.job_id.clone(),
            job_execution_id: self.job.job_execution_id.clone(),
            status: status as i32,
            std_out: String::new(),
            std_err: String::new(),
        }
    }

    /// Helper method to create a `JobExecutionUpdate` with the appropriate
    /// `job_id` and `job_execution_id`.
    /// ```ignore
    /// pub async fn handler(ctx: Ctx) -> Result<JobExecutionUpdate> {
    ///    println!("i ran!");
    ///    Ok(ctx.success().stdout("yay!"))
    /// }
    /// ```
    pub fn success(&self) -> JobExecutionUpdate {
        self.status(JobExecutionStatus::Succeeded)
    }

    /// Helper method to create a `JobExecutionUpdate` with the appropriate
    /// `job_id` and `job_execution_id`.
    /// ```ignore
    /// pub async fn handler(ctx: Ctx) -> Result<JobExecutionUpdate> {
    ///    println!("i did not run properly!");
    ///    Ok(ctx.failure().stderr("oh no!"))
    /// }
    /// ```
    pub fn failure(&self) -> JobExecutionUpdate {
        self.status(JobExecutionStatus::Failed)
    }

    /// Helper method to create a `JobExecutionUpdate` with the appropriate
    /// `job_id` and `job_execution_id`.
    /// ```ignore
    /// pub async fn handler(ctx: Ctx) -> Result<JobExecutionUpdate> {
    ///    Ok(ctx.cancelled().stdout("cancelled job"))
    /// }
    /// ```
    pub fn cancelled(&self) -> JobExecutionUpdate {
        self.status(JobExecutionStatus::Cancelled)
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
    pub fn args(&self) -> Vec<String> {
        let Some(args) = &self.job_args else {
            return vec![];
        };

        args.split(" ")
            .filter(|x| !x.trim().is_empty())
            .map(String::from)
            .collect()
    }

    /// If command follows the pattern of "<command> <json>", will attempt to
    /// deserialize the json part of the payload into the type passed as an argument.
    pub fn args_json<'a, T>(&'a self) -> color_eyre::Result<T>
    where
        T: Deserialize<'a>,
    {
        let args = self
            .job_args
            .as_ref()
            .wrap_err("no args provided to parse as json")?
            .as_str();

        let json = serde_json::from_str(args)?;

        Ok(json)
    }

    pub fn args_raw(&self) -> Option<&str> {
        self.job_args.as_deref()
    }

    pub async fn force_relay_reconnect(&self) -> color_eyre::Result<()> {
        self.job_client.force_relay_reconnect().await
    }
}

/// A set of extensions to make life easier when creating the `JobExecutionUpdate` struct.
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
