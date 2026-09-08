use super::{client::JobClient, handler::Handler, sanitize::redact_job_document};
use crate::program::Deps;
use bon::bon;
use color_eyre::eyre::{eyre, ContextCompat};
use orb_relay_messages::jobs::v1::{
    JobExecution, JobExecutionStatus, JobExecutionUpdate,
};
use serde::Deserialize;
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;
use tracing::{error, warn};

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
                let Some(command) = get_zoci_command(&ctx.job.job_document) else {
                    let update = ctx.status(JobExecutionStatus::FailedUnsupported);

                    if let Err(e) = ctx.job_client.send_job_update(&update).await {
                        error!(
                            job_execution_id = %ctx.job.job_execution_id,
                            job_id = %ctx.job.job_id,
                            job_document = %redact_job_document(&ctx.job.job_document),
                            error = ?e,
                            "failed to send job update for FailedUnsupported job"
                        );
                    }

                    return None;
                };

                let handler = zoci_handler();
                (command, handler)
            }

            Some((c, h)) => (c.to_owned(), h),
        };

        ctx.job_args = ctx
            .job
            .job_document
            .split_once(&command)
            .map(|(_cmd, args_raw)| args_raw.trim())
            .filter(|args_raw| !args_raw.is_empty())
            .map(String::from);

        ctx.cmd.push_str(&command);

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

/// Max length of a zoci command, in bytes.
const ZOCI_CMD_MAX_LEN: usize = 64;

fn get_zoci_command(full_cmd: &str) -> Option<String> {
    let cmd = full_cmd
        .split_once(" ")
        .map(|(cmd, _)| cmd)
        .unwrap_or(full_cmd);

    // the command is interpolated into a zenoh key expression (see
    // `zoci_handler`), so anything outside the charset real queryables use is
    // unsupported -- notably `*` and `**`, which would fan a single job out to
    // every `job/*` queryable.
    let is_valid = (1..=ZOCI_CMD_MAX_LEN).contains(&cmd.len())
        && cmd
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');

    if !is_valid && !cmd.is_empty() {
        // distinguishable from a benign unknown command; `?` escapes control
        // characters and the token is truncated since it is attacker-chosen
        warn!(
            token = ?cmd.chars().take(ZOCI_CMD_MAX_LEN).collect::<String>(),
            "rejected zoci command outside the [a-z0-9_] charset"
        );
    }

    is_valid.then(|| cmd.to_owned())
}

fn zoci_handler() -> Handler {
    async fn handler(ctx: Ctx) -> color_eyre::Result<JobExecutionUpdate> {
        let topic = format!("**/job/{}", ctx.cmd);
        tracing::info!("zoci topic: {}", topic);

        let replies = ctx
            .deps
            .zenorb
            .get(&topic)
            .timeout(Duration::from_secs(30))
            .payload(ctx.args_raw().unwrap_or_default())
            .await
            .map_err(|e| {
                eyre!("failed to execute zoci command on topic: {topic}. err: {e}")
            })?;

        let reply = match replies.recv_async().await {
            Ok(reply) => reply.into_result(),
            Err(_) => return Ok(ctx.status(JobExecutionStatus::FailedUnsupported)),
        };

        let res = match reply {
            Err(err) => {
                let payload = err.payload().to_bytes();
                let stderr = String::from_utf8_lossy(&payload);

                if stderr == "null" {
                    ctx.failure()
                } else {
                    ctx.failure().stderr(stderr.to_string())
                }
            }

            Ok(sample) => {
                let payload = sample.payload().to_bytes();
                let stdout = String::from_utf8_lossy(&payload);

                if stdout == "null" {
                    ctx.success()
                } else {
                    ctx.success().stdout(stdout.to_string())
                }
            }
        };

        Ok(res)
    }

    Arc::new(|ctx| Box::pin(handler(ctx)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_commands_matching_real_zoci_endpoints() {
        assert_eq!(get_zoci_command("wifi_add").as_deref(), Some("wifi_add"));
        assert_eq!(get_zoci_command("gondor").as_deref(), Some("gondor"));
        assert_eq!(
            get_zoci_command("wifi_scan --json").as_deref(),
            Some("wifi_scan")
        );
        assert_eq!(
            get_zoci_command("read_temp sensor-a nominal").as_deref(),
            Some("read_temp")
        );

        let max_len = "a".repeat(ZOCI_CMD_MAX_LEN);
        assert_eq!(
            get_zoci_command(&max_len).as_deref(),
            Some(max_len.as_str())
        );
    }

    #[test]
    fn rejects_commands_outside_the_charset() {
        let too_long = "a".repeat(ZOCI_CMD_MAX_LEN + 1);

        for cmd in [
            "*",
            "**",
            "a/b",
            "wifi_add$x",
            "Wifi_Add",
            "..",
            "",
            // only a single space delimits args, so whitespace-separated
            // documents fail closed as one oversized token
            "wifi_add\tsome-arg",
            &too_long,
        ] {
            assert_eq!(get_zoci_command(cmd), None, "expected {cmd:?} rejected");
        }
    }
}
