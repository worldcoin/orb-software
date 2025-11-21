use crate::{
    connd,
    job_system::ctx::{Ctx, JobExecutionUpdateExt},
};
use color_eyre::{eyre::bail, Result};
use orb_connd_dbus::ConndProxy;
use orb_relay_messages::jobs::v1::JobExecutionUpdate;
use std::time::Duration;

/// command format: `wifi_connect <ssid>`
/// example:
/// wifi_connect TFHOrbs
#[tracing::instrument(skip(ctx))]
pub async fn handler(ctx: Ctx) -> Result<JobExecutionUpdate> {
    let Some(ssid) = ctx.args_raw() else {
        bail!("ssid must be provided as an argument")
    };

    let connd = ConndProxy::new(&ctx.deps().session_dbus).await?;
    let network = connd::connect_to_wifi_and_wait_for_internet(
        &connd,
        ssid,
        Duration::from_secs(10),
    )
    .await?;

    ctx.force_relay_reconnect().await?;

    let res = serde_json::to_string(&network)?;

    Ok(ctx.success().stdout(res))
}
