use crate::job_system::ctx::{Ctx, JobExecutionUpdateExt};
use color_eyre::{
    eyre::{bail, Context},
    Result,
};
use orb_relay_messages::jobs::v1::JobExecutionUpdate;
use tracing::info;

/// command format: `gondor-calls-for-ota ${target_version}`
#[tracing::instrument(skip(ctx))]
pub async fn handler(ctx: Ctx) -> Result<JobExecutionUpdate> {
    let args = ctx.args();
    let Some(version) = args.first() else {
        return Ok(ctx.failure().stderr("Missing target version"));
    };

    info!("Updating to {} for job {}", version, ctx.execution_id());

    let output = ctx
        .deps()
        .shell
        .exec(&["/usr/local/bin/gondor-calls-for-ota", version])
        .await
        .context("failed to spawn gondor")?
        .wait_with_output()
        .await
        .context("failed to wait for gondor")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("gondor-calls-for-ota failed: {stderr}");
    }

    Ok(ctx.success())
}
