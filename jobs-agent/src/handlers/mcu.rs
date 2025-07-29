use crate::job_system::ctx::Ctx;
use color_eyre::Result;
use orb_relay_messages::jobs::v1::JobExecutionUpdate;

/// command format: `mcu <mcu_variant> <cmd>`
///
/// `mcu_variant` options: `main` | `sec`
///
/// `cmd` options: `reboot`
pub async fn handler(_ctx: Ctx) -> Result<JobExecutionUpdate> {
    todo!()
}
