use crate::job_system::ctx::Ctx;
use color_eyre::{eyre::ensure, Result};
use orb_connd_dbus::ConndProxy;
use orb_relay_messages::jobs::v1::JobExecutionUpdate;

/// command format: `wifi_connect <ssid>`
/// example:
/// wifi_connect TFHOrbs
#[tracing::instrument(skip(ctx))]
pub async fn handler(ctx: Ctx) -> Result<JobExecutionUpdate> {
    ensure!(
        ctx.args().len() == 1,
        "Expected 1 arguments, got {}",
        ctx.args().len()
    );

    let ssid = ctx.args()[0].to_owned();

    let connd = ConndProxy::new(&ctx.deps().session_dbus).await?;
    connd.connect_to_wifi(ssid).await?;

    Ok(ctx.success())
}
