use crate::job_system::ctx::Ctx;
use color_eyre::{eyre::bail, Result};
use tracing::info;

#[derive(Clone, Copy, Debug)]
pub enum ServiceAction {
    Start,
    Stop,
    Restart,
    Status,
}

impl ServiceAction {
    pub fn from_str(s: &str) -> Result<Self> {
        match s {
            "start" => Ok(Self::Start),
            "stop" => Ok(Self::Stop),
            "restart" => Ok(Self::Restart),
            "status" => Ok(Self::Status),
            _ => bail!(
                "Invalid action: '{s}'. Must be one of: start, stop, restart, status"
            ),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Restart => "restart",
            Self::Status => "status",
        }
    }

    fn step_name(self, service_name: &str) -> String {
        let verb = match self {
            Self::Start => "starting",
            Self::Stop => "stopping",
            Self::Restart => "restarting",
            Self::Status => "checking status of",
        };

        format!("{verb} {service_name}")
    }
}

pub async fn run_service_action(
    ctx: &Ctx,
    action: ServiceAction,
    service_name: &str,
) -> Result<String> {
    let step_name = action.step_name(service_name);
    let child = ctx
        .deps()
        .shell
        .exec(&["systemctl", action.as_str(), service_name])
        .await?;

    let output = child.wait_with_output().await?;

    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&output.stderr);

        let status = output.status.code().map_or_else(
            || "terminated by signal".to_string(),
            |code| format!("exit status {code}"),
        );

        if stderr.trim().is_empty() && stdout.is_empty() {
            bail!("{step_name} failed with {status}");
        }

        if stderr.trim().is_empty() {
            bail!("{step_name} failed with {status}: stdout: {stdout}");
        }

        bail!(
            "{step_name} failed with {status}: stderr: {}",
            stderr.trim()
        );
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

pub async fn stop_service(ctx: &Ctx, service_name: &str) -> Result<()> {
    let _ = run_service_action(ctx, ServiceAction::Stop, service_name).await?;

    Ok(())
}

pub async fn start_service(ctx: &Ctx, service_name: &str) -> Result<()> {
    let _ = run_service_action(ctx, ServiceAction::Start, service_name).await?;

    Ok(())
}

pub async fn restart_service(
    ctx: &Ctx,
    service_name: &str,
    reason: &str,
) -> Result<()> {
    info!("Restarting {service_name} to apply {reason}");
    let _ = run_service_action(ctx, ServiceAction::Restart, service_name).await?;

    Ok(())
}
