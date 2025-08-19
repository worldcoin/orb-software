use crate::job_system::ctx::Ctx;
use color_eyre::{eyre::eyre, Result};
use orb_relay_messages::jobs::v1::JobExecutionUpdate;

/// command format: `read_gimbal`
#[tracing::instrument]
pub async fn handler(_ctx: Ctx) -> Result<JobExecutionUpdate> {
    Err(eyre!("not yet implemented"))
}
