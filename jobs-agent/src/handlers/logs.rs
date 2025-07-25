use crate::job_system::ctx::{Ctx, JobExecutionUpdateExt};
use color_eyre::{
    eyre::{bail, Context, ContextCompat},
    Result,
};
use orb_relay_messages::jobs::v1::JobExecutionUpdate;
use std::time::Duration;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    time::Instant,
};
use tracing::info;

#[derive(Debug)]
enum LogAction {
    Tail { service: String },
    Lines { number: usize, service: String },
}

impl LogAction {
    fn from_args(args: &[String]) -> Result<LogAction> {
        if args.len() != 2 {
            bail!(
                "incorrect number of arguments, expected 2, got: {}",
                args.len()
            );
        }

        let service = args[1].to_owned();

        if args[0] == "tail" {
            return Ok(LogAction::Tail { service });
        }

        let number: usize = args[0].parse().wrap_err_with(|| {
            format!(
                "expected first arg to be the number of lines to print, instead got {}",
                args[0]
            )
        })?;

        Ok(LogAction::Lines { number, service })
    }
}

#[tracing::instrument]
pub async fn handler(ctx: Ctx) -> Result<JobExecutionUpdate> {
    let log_action = LogAction::from_args(ctx.args())?;

    info!(
        "Tailing logs: {:?} for job {}",
        log_action,
        ctx.execution_id()
    );

    match log_action {
        LogAction::Lines { number, service } => {
            let output = ctx
                .deps()
                .shell
                .exec(&["journalctl", "-u", &service, "-n", &number.to_string()])
                .await?
                .wait_with_output()
                .await?;

            let stdout = String::from_utf8_lossy(&output.stdout).to_string();

            return Ok(ctx.success().stdout(stdout));
        }

        LogAction::Tail { service } => {
            let now = Instant::now();
            let max_duration = Duration::from_secs(5 * 60);

            // send initial progress
            let _ = ctx.progress().send().await;
            let mut child_proc = ctx
                .deps()
                .shell
                .exec(&["journalctl", "-f", "-u", &service])
                .await?;

            let out = child_proc
                .stdout
                .take()
                .wrap_err("could not get stdout from proc")?;

            let mut reader = BufReader::new(out);
            let mut line = String::new();

            loop {
                if now.elapsed() > max_duration && ctx.is_cancelled() {
                    return Ok(ctx.success().stderr("cancelled"));
                }

                let bytes_read = reader.read_line(&mut line).await?;
                if bytes_read == 0 {
                    break;
                }

                // TOOD: handle error
                let _ = ctx.progress().stdout(line.clone()).send().await;
            }
        }
    }

    Ok(ctx.success())
}
