use crate::job_system::ctx::Ctx;
use color_eyre::{
    eyre::{bail, eyre},
    Result,
};
use orb_relay_messages::jobs::v1::{JobExecutionStatus, JobExecutionUpdate};
use std::{future::Future, path::Path};
use tokio::{fs, io};
use tracing::info;

/// command format: `reboot`
#[tracing::instrument(skip(ctx))]
pub async fn handler(ctx: Ctx) -> Result<JobExecutionUpdate> {
    run_reboot_flow(ctx, "reboot", |_ctx| async move {
        Ok(RebootPlan::with_stdout("rebooting\n"))
    })
    .await
}

#[derive(Debug, Default)]
pub struct RebootPlan {
    pub progress_stdout: Option<String>,
    pub progress_stderr: Option<String>,
}

impl RebootPlan {
    pub fn with_stdout(stdout: impl Into<String>) -> Self {
        Self {
            progress_stdout: Some(stdout.into()),
            ..Self::default()
        }
    }

    #[allow(dead_code)]
    pub fn with_stderr(stderr: impl Into<String>) -> Self {
        Self {
            progress_stderr: Some(stderr.into()),
            ..Self::default()
        }
    }
}

pub async fn run_reboot_flow<F, Fut>(
    ctx: Ctx,
    command_label: &str,
    on_reboot: F,
) -> Result<JobExecutionUpdate>
where
    F: FnOnce(Ctx) -> Fut,
    Fut: Future<Output = Result<RebootPlan>>,
{
    info!(
        "Handling {command_label} command for job {}",
        ctx.execution_id()
    );
    let store_path = &ctx.deps().settings.store_path;
    let reboot_status = RebootStatus::from_lockfile(store_path).await?;

    match reboot_status {
        RebootStatus::Pending(job_execution_id)
            if job_execution_id == ctx.execution_id() =>
        {
            info!("Orb rebooted due to job execution {:?}", ctx.execution_id());
            RebootStatus::remove_pending_lockfile(store_path).await?;

            ctx.progress()
                .stdout("rebooted")
                .send()
                .await
                .map_err(|e| eyre!("failed to send progress {e:?}"))?;

            Ok(ctx.success())
        }

        // we are free to do a reboot
        _ => {
            info!(
                "Orb rebooting due to job execution {:?}",
                ctx.execution_id()
            );

            let plan = on_reboot(ctx.clone()).await?;

            RebootStatus::write_pending_lockfile(store_path, ctx.execution_id())
                .await?;

            let progress_builder = ctx.progress().stdout(
                plan.progress_stdout
                    .unwrap_or_else(|| "rebooting\n".to_string()),
            );

            if let Some(stderr) = plan.progress_stderr {
                progress_builder
                    .stderr(stderr)
                    .send()
                    .await
                    .map_err(|e| eyre!("failed to send progress {e:?}"))?;
            } else {
                progress_builder
                    .send()
                    .await
                    .map_err(|e| eyre!("failed to send progress {e:?}"))?;
            }

            execute_reboot(&ctx).await?;

            Ok(ctx.status(JobExecutionStatus::InProgress))
        }
    }
}

async fn execute_reboot(ctx: &Ctx) -> Result<()> {
    let out = ctx
        .deps()
        .shell
        .exec(&["orb-mcu-util", "reboot", "--delay", "30", "orb"])
        .await?
        .wait_with_output()
        .await?;

    if !out.status.success() {
        bail!(
            "orb-mcu-util reboot failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    let out = ctx
        .deps()
        .shell
        .exec(&["shutdown", "now"])
        .await?
        .wait_with_output()
        .await?;

    if !out.status.success() {
        bail!(
            "shutdown now failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    Ok(())
}

#[derive(Debug)]
pub enum RebootStatus {
    /// There is a pending reboot. If this file exists the reboot was most likely executed.
    Pending(String),
    /// We are free to perform a reboot
    Free,
}

impl RebootStatus {
    const FILENAME: &str = "reboot.lock";

    pub async fn from_lockfile(store: impl AsRef<Path>) -> Result<Self> {
        match fs::read_to_string(store.as_ref().join(Self::FILENAME)).await {
            Ok(s) => Ok(RebootStatus::Pending(s)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(RebootStatus::Free),
            Err(e) => Err(e.into()),
        }
    }

    async fn write_pending_lockfile(
        store: impl AsRef<Path>,
        job_execution_id: impl Into<String>,
    ) -> Result<()> {
        fs::write(store.as_ref().join(Self::FILENAME), job_execution_id.into()).await?;
        Ok(())
    }

    pub async fn remove_pending_lockfile(store: impl AsRef<Path>) -> Result<()> {
        fs::remove_file(store.as_ref().join(Self::FILENAME)).await?;
        Ok(())
    }
}
