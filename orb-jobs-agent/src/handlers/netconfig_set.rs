use crate::job_system::ctx::{Ctx, JobExecutionUpdateExt};
use color_eyre::Result;
use orb_connd_dbus::ConndProxy;
use orb_relay_messages::jobs::v1::JobExecutionUpdate;
use serde::{Deserialize, Serialize};
use serde_json::json;

/// command format: `netconfig_set <NetConfig json>`
///
/// struct NetConfig {
///     wifi: bool,
///     smart_switching: bool,
///     airplane_mode: bool,
/// }
///
/// example:
/// netconfig_set {"wifi":bool,"smart_switching":bool,"airplane_mode":bool}
#[tracing::instrument(skip(ctx))]
pub async fn handler(ctx: Ctx) -> Result<JobExecutionUpdate> {
    let NetConfig {
        wifi,
        smart_switching,
        airplane_mode,
    } = ctx.args_json()?;

    let connd = ConndProxy::new(&ctx.deps().session_dbus).await?;
    let netcfg = connd
        .netconfig_set(wifi, smart_switching, airplane_mode)
        .await?;

    let res = json!({
        "wifi": netcfg.wifi,
        "smart_switching": netcfg.smart_switching,
        "airplane_mode": netcfg.airplane_mode
    });

    Ok(ctx.success().stdout(res.to_string()))
}

#[derive(Deserialize, Serialize)]
struct NetConfig {
    wifi: bool,
    smart_switching: bool,
    airplane_mode: bool,
}
