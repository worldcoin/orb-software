use crate::job_system::ctx::{Ctx, JobExecutionUpdateExt};
use color_eyre::Result;
use orb_connd_dbus::ConndProxy;
use orb_relay_messages::jobs::v1::JobExecutionUpdate;
use serde_json::json;

/// command format: `netconfig_get`
///
/// example:
/// netconfig_get
#[tracing::instrument(skip(ctx))]
pub async fn handler(ctx: Ctx) -> Result<JobExecutionUpdate> {
    let connd = ConndProxy::new(&ctx.deps().session_dbus).await?;
    let netcfg = connd.netconfig_get().await?;

    let res = json!({
        "wifi": netcfg.wifi,
        "smart_switching": netcfg.smart_switching,
        "airplane_mode": netcfg.airplane_mode
    });

    Ok(ctx.success().stdout(res.to_string()))
}
