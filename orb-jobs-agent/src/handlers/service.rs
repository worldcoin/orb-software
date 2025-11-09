use crate::job_system::ctx::{Ctx, JobExecutionUpdateExt};
use color_eyre::{
    eyre::{bail, ensure},
    Result,
};
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

    let child = ctx
        .deps()
        .shell
        .exec(&["systemctl", action_str, service_name])
        .await?;

    let output = child.wait_with_output().await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "systemctl {} {} failed: {}",
            action_str,
            service_name,
            stderr
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();

    Ok(ctx.success().stdout(stdout))
}

#[derive(Debug)]
enum ServiceAction {
    Start,
    Stop,
    Restart,
    Status,
}

impl ServiceAction {
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "start" => Ok(ServiceAction::Start),
            "stop" => Ok(ServiceAction::Stop),
            "restart" => Ok(ServiceAction::Restart),
            "status" => Ok(ServiceAction::Status),
            _ => bail!(
                "Invalid action: '{s}'. Must be one of: start, stop, restart, status"
            ),
        }
    }

    fn as_str(&self) -> &str {
        match self {
            ServiceAction::Start => "start",
            ServiceAction::Stop => "stop",
            ServiceAction::Restart => "restart",
            ServiceAction::Status => "status",
        }
    }
}
