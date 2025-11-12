use crate::job_system::ctx::{Ctx, JobExecutionUpdateExt};
use color_eyre::Result;
use orb_connd_dbus::ConndProxy;
use orb_relay_messages::jobs::v1::JobExecutionUpdate;
use serde_json::json;

/// command format: `wifi_list`
/// example:
/// wifi_list
#[tracing::instrument(skip(ctx))]
pub async fn handler(ctx: Ctx) -> Result<JobExecutionUpdate> {
    let connd = ConndProxy::new(&ctx.deps().session_dbus).await?;
    let profiles: Vec<_> = connd
        .list_wifi_profiles()
        .await?
        .into_iter()
        .map(|profile| {
            json!({
                "ssid": profile.ssid,
                "sec": profile.sec,
                "is_active": profile.is_active
            })
        })
        .collect();

    let profiles = serde_json::to_string(&profiles)?;

    Ok(ctx.success().stdout(profiles))
}
