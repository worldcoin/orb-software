use crate::job_system::ctx::{Ctx, JobExecutionUpdateExt};
use color_eyre::{
    eyre::{bail, Context},
    Result,
};
use orb_relay_messages::jobs::v1::{JobExecutionStatus, JobExecutionUpdate};

/// command format: `mcu`
#[tracing::instrument(skip(ctx))]
pub async fn handler(ctx: Ctx) -> Result<JobExecutionUpdate> {
    let has_args = ctx.args().iter().any(|arg| !arg.trim().is_empty());

    if has_args {
        return Ok(ctx
            .status(JobExecutionStatus::FailedUnsupported)
            .stderr("mcu job does not accept arguments"));
    }

    let output = ctx
        .deps()
        .shell
        .exec(&["orb-mcu-util", "info"])
        .await
        .context("failed to spawn orb-mcu-util info")?
        .wait_with_output()
        .await
        .context("failed to wait for orb-mcu-util info")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("orb-mcu-util info failed: {stderr}");
    }

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    Ok(ctx.success().stdout(stdout))
}
