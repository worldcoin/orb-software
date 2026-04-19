use super::service_control::{self, ServiceAction};
use crate::job_system::ctx::{Ctx, JobExecutionUpdateExt};
use color_eyre::{eyre::ensure, Result};
use orb_relay_messages::jobs::v1::JobExecutionUpdate;
use tracing::info;

/// command format: service <action> <service_name>
///
/// action options: "start" | "stop" | "restart" | "status"
///
/// service_name: name of the systemd service
///
/// examples:
///
/// service stop worldcoin-core.service
///
/// service restart orb-core.service
#[tracing::instrument(skip(ctx))]
pub async fn handler(ctx: Ctx) -> Result<JobExecutionUpdate> {
    let args = ctx.args();
    ensure!(
        args.len() == 2,
        "Expected 2 arguments (action service_name), got {}",
        args.len()
    );

    let action = ServiceAction::from_str(&args[0])?;
    let service_name = &args[1];
    let action_str = action.as_str();

    info!(
        "Executing systemctl {} {} for job {}",
        action_str,
        service_name,
        ctx.execution_id()
    );

    let stdout =
        service_control::run_service_action(&ctx, action, service_name).await?;

    Ok(ctx.success().stdout(stdout))
}
