use crate::{job_system::ctx::Ctx, reboot};
use color_eyre::Result;
use orb_relay_messages::jobs::v1::JobExecutionUpdate;

/// command format: `reboot`
#[tracing::instrument(skip(ctx))]
pub async fn handler(ctx: Ctx) -> Result<JobExecutionUpdate> {
    reboot::run_reboot_flow(ctx, "reboot", |_ctx| async move {
        Ok(reboot::RebootPlan::with_stdout("rebooting\n"))
    })
    .await
}
