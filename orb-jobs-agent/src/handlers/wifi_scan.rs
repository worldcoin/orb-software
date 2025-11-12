use crate::job_system::ctx::{Ctx, JobExecutionUpdateExt};
use color_eyre::Result;
use orb_connd_dbus::ConndProxy;
use orb_relay_messages::jobs::v1::JobExecutionUpdate;

/// command format: `wifi_scan`
/// example:
/// wifi_scan
#[tracing::instrument(skip(ctx))]
pub async fn handler(ctx: Ctx) -> Result<JobExecutionUpdate> {
    let connd = ConndProxy::new(&ctx.deps().session_dbus).await?;
    let aps = connd.scan_wifi().await?;
    let aps = serde_json::to_string(&aps)?;

    Ok(ctx.success().stdout(aps))
}
