use crate::job_system::ctx::Ctx;
use color_eyre::{eyre::ensure, Result};
use orb_connd_dbus::ConndProxy;
use orb_relay_messages::jobs::v1::JobExecutionUpdate;

/// command format: `wifi_remove <ssid>`
/// example:
/// wifi_remove TFHOrbs
#[tracing::instrument(skip(ctx))]
pub async fn handler(ctx: Ctx) -> Result<JobExecutionUpdate> {
    ensure!(
        ctx.args().len() == 1,
        "Expected 1 arguments, got {}",
        ctx.args().len()
    );

    let ssid = &ctx.args()[0];

    let connd = ConndProxy::new(&ctx.deps().session_dbus).await?;
    connd.remove_wifi_profile(ssid.to_owned()).await?;

    Ok(ctx.success())
}
