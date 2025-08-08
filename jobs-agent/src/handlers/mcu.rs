use crate::job_system::ctx::Ctx;
use color_eyre::{eyre::eyre, Result};
use orb_relay_messages::jobs::v1::JobExecutionUpdate;

/// command format: `mcu <mcu_variant> <cmd>`
///
/// `mcu_variant` options: `main` | `sec`
///
/// `cmd` options: `reboot`
#[tracing::instrument]
pub async fn handler(ctx: Ctx) -> Result<JobExecutionUpdate> {
    Err(eyre!("not yet implemented"))
}
