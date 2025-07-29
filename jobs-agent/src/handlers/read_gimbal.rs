use crate::job_system::ctx::Ctx;
use color_eyre::Result;
use orb_relay_messages::jobs::v1::JobExecutionUpdate;

/// command format: `read_gimbal`
#[tracing::instrument]
pub async fn handler(_ctx: Ctx) -> Result<JobExecutionUpdate> {
    todo!()
}
