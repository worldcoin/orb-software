use crate::job_system::ctx::{Ctx, JobExecutionUpdateExt};
use color_eyre::{eyre::Context, Result};
use orb_relay_messages::jobs::v1::{JobExecutionStatus, JobExecutionUpdate};

/// command format: `se050_list_keys`
#[tracing::instrument(skip(ctx))]
pub async fn handler(ctx: Ctx) -> Result<JobExecutionUpdate> {
    if ctx.args().iter().any(|arg| !arg.trim().is_empty()) {
        return Ok(ctx
            .status(JobExecutionStatus::FailedUnsupported)
            .stderr("se050_list_keys job does not accept arguments"));
    }

    let output = ctx
        .deps()
        .shell
        .exec(&[
            "/usr/bin/env",
            "--chdir=/usr/persistent/se",
            "/usr/bin/01_list_keys",
        ])
        .await
        .context("failed to spawn 01_list_keys")?
        .wait_with_output()
        .await
        .context("failed to wait for 01_list_keys")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let update = if output.status.success() {
        ctx.success()
    } else {
        ctx.failure()
    };

    Ok(update.stdout(stdout).stderr(stderr))
}
