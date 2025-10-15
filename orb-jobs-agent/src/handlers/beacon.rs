use crate::job_system::ctx::{Ctx, JobExecutionUpdateExt};
use color_eyre::{eyre::{bail, WrapErr}, Result};
use orb_relay_messages::jobs::v1::JobExecutionUpdate;

/// command format: `beacon`
#[tracing::instrument(skip(ctx))]
pub async fn handler(ctx: Ctx) -> Result<JobExecutionUpdate> {
    let output = ctx
        .deps()
        .shell
        .exec(&["orb-beacon"])
        .await
        .wrap_err("failed to spawn orb-beacon")?
        .wait_with_output()
        .await
        .wrap_err("failed to get orb-beacon output")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("orb-beacon failed: {stderr}");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let mut update = ctx.success();

    if !stdout.is_empty() {
        update = update.stdout(stdout);
    }

    if !stderr.is_empty() {
        update = update.stderr(stderr);
    }

    Ok(update)
}
