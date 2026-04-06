use super::service_control;
use crate::job_system::ctx::{Ctx, JobExecutionUpdateExt};
use color_eyre::{
    eyre::{bail, ensure},
    Result,
};
use orb_endpoints::Backend;
use orb_relay_messages::jobs::v1::JobExecutionUpdate;
use tracing::info;

const CORE_SERVICE: &str = "worldcoin-core.service";

/// command format: `skip_operator_qr`
///
/// Sets `EXTRA_ARGS="-o"` and restarts worldcoin-core.service to skip
/// operator QR code scanning. Only allowed on staging orbs.
///
/// To restore default behavior: `service restart worldcoin-core.service`
#[tracing::instrument(skip(ctx))]
pub async fn handler(ctx: Ctx) -> Result<JobExecutionUpdate> {
    ensure!(
        ctx.args().is_empty(),
        "Expected no arguments, got {}",
        ctx.args().len()
    );

    let backend = Backend::from_env().ok();
    if backend != Some(Backend::Staging) {
        bail!(
            "skip_operator_qr is only allowed on staging orbs (got: {:?})",
            backend
        );
    }

    info!("Setting EXTRA_ARGS=-o for job {}", ctx.execution_id());

    let child = ctx
        .deps()
        .shell
        .exec(&["systemctl", "set-environment", "EXTRA_ARGS=-o"])
        .await?;
    let output = child.wait_with_output().await?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("Failed to set EXTRA_ARGS: {}", stderr.trim());
    }

    service_control::restart_service(&ctx, CORE_SERVICE, "operator QR skip").await?;

    Ok(ctx
        .success()
        .stdout("orb-core restarted with operator QR skip"))
}
