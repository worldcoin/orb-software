use crate::job_system::ctx::{Ctx, JobExecutionUpdateExt};
use color_eyre::{eyre::Context, Result};
use orb_relay_messages::jobs::v1::JobExecutionUpdate;

/// command format: `check_my_orb`
#[tracing::instrument]
pub async fn handler(ctx: Ctx) -> Result<JobExecutionUpdate> {
    let output = ctx
        .deps()
        .shell
        .exec(&["check-my-orb"])
        .await
        .wrap_err("failed to spawn check_my_orb")?
        .wait_with_output()
        .await
        .wrap_err("failed to get output for check-my-orb")?;

    let output = String::from_utf8_lossy(&output.stdout).to_string();

    Ok(ctx.success().stdout(output))
}
