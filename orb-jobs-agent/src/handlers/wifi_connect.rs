use crate::job_system::ctx::Ctx;
use color_eyre::{eyre::bail, Result};
use orb_connd_dbus::ConndProxy;
use orb_relay_messages::jobs::v1::JobExecutionUpdate;

/// command format: `wifi_connect <ssid>`
/// example:
/// wifi_connect TFHOrbs
#[tracing::instrument(skip(ctx))]
pub async fn handler(ctx: Ctx) -> Result<JobExecutionUpdate> {
    let Some(ssid) = ctx.args_raw() else {
        bail!("ssid must be provided as an argument")
    };

    let connd = ConndProxy::new(&ctx.deps().session_dbus).await?;
    connd.connect_to_wifi(ssid.into()).await?;

    Ok(ctx.success())
}
