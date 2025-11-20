use crate::job_system::ctx::{Ctx, JobExecutionUpdateExt};
use color_eyre::{eyre::Context, Result};
use orb_relay_messages::jobs::v1::JobExecutionUpdate;
use tracing::info;

/// command format: `fsck ${device_path}`
#[tracing::instrument(skip(ctx))]
pub async fn handler(ctx: Ctx) -> Result<JobExecutionUpdate> {
    let args = ctx.args();
    let device = if let Some(d) = args.first() {
        d
    } else {
        return Ok(ctx.failure().stderr("Missing device argument"));
    };

    info!("Running fsck on {} for job {}", device, ctx.execution_id());

    if let Ok(mut child) = ctx.deps().shell.exec(&["umount", device]).await {
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), child.wait()).await;
    }

    let output = ctx
        .deps()
        .shell
        .exec(&["fsck", "-y", "-f", device])
        .await
        .context("failed to spawn fsck")?
        .wait_with_output()
        .await
        .context("failed to wait for fsck")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let message = format!("STDOUT:\n{}\nSTDERR:\n{}", stdout, stderr);

    // fsck exit codes:
    // 0 - No errors
    // 1 - File system errors corrected
    // 2 - System should be rebooted
    // ...
    if let Some(code) = output.status.code() {
        if code == 0 || code == 1 {
            return Ok(ctx.success().stdout(message));
        }
    }

    Ok(ctx.failure().stdout(message))
}
