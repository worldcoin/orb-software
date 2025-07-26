use crate::job_system::ctx::Ctx;
use color_eyre::Result;
use orb_relay_messages::jobs::v1::JobExecutionUpdate;

pub async fn handler(_ctx: Ctx) -> Result<JobExecutionUpdate> {
    todo!()
}
