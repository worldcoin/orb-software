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
/// Stops worldcoin-core.service and starts orb-core with `-o` to skip
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

    info!("Stopping {} for job {}", CORE_SERVICE, ctx.execution_id());

    service_control::stop_service(&ctx, CORE_SERVICE).await?;

    info!("Starting orb-core with -o for job {}", ctx.execution_id());

    // orb-core is long-running; spawn it and return immediately.
    let _child = ctx.deps().shell.exec(&["orb-core", "-o", ""]).await?;

    Ok(ctx.success().stdout("orb-core started with operator QR skip"))
}
