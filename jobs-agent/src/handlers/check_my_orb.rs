use crate::job_system::ctx::{Ctx, JobExecutionUpdateExt};
use color_eyre::{eyre::ContextCompat, Result};
use orb_relay_messages::jobs::v1::JobExecutionUpdate;
use tokio::io::AsyncReadExt;

/// command format: `check_my_orb`
pub async fn handler(ctx: Ctx) -> Result<JobExecutionUpdate> {
    let mut output = ctx
        .deps()
        .shell
        .exec(&["check-my-orb"])
        .await?
        .stdout
        .wrap_err("failed to get stdout for check-my-orb")?;

    let mut bytes = Vec::new();
    let _ = output.read(&mut bytes).await?;
    let output = String::from_utf8_lossy(&bytes);

    Ok(ctx.success().stdout(output.to_string()))
}
