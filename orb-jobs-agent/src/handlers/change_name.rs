use crate::job_system::ctx::{Ctx, JobExecutionUpdateExt};
use color_eyre::{eyre::ensure, Result};
use orb_relay_messages::jobs::v1::JobExecutionUpdate;
use tracing::info;

/// command format: `change_name <orb-name>`
/// example: change_name silly-philly
#[tracing::instrument(skip(ctx))]
pub async fn handler(ctx: Ctx) -> Result<JobExecutionUpdate> {
    ensure!(
        ctx.args().len() == 1,
        "Expected 1 argument, got {}",
        ctx.args().len()
    );

    let orb_name = &ctx.args()[0];

    ensure!(
        orb_name.contains('-'),
        "orb-name must be in the format 'something-something' (must contain a dash)"
    );

    info!(
        "Setting orb name to: {} for job {}",
        orb_name,
        ctx.execution_id()
    );

    let orb_name_path = &ctx.deps().settings.orb_name_path;
    tokio::fs::write(orb_name_path, orb_name).await?;

    Ok(ctx.success().stdout(format!("Orb name set to: {orb_name}")))
}
