use crate::job_system::ctx::{Ctx, JobExecutionUpdateExt};
use color_eyre::Result;
use orb_info::{OrbJabilId, OrbName};
use orb_relay_messages::jobs::v1::JobExecutionUpdate;

#[tracing::instrument]
pub async fn handler(ctx: Ctx) -> Result<JobExecutionUpdate> {
    let orb_name = OrbName::read()
        .await
        .unwrap_or(OrbName("NO_ORB_NAME".to_string()));

    let jabil_id = OrbJabilId::read()
        .await
        .unwrap_or(OrbJabilId("NO_JABIL_ID".to_string()));

    let details = serde_json::json!({
        "orb_name": orb_name.to_string(),
        "jabil_id": jabil_id.to_string(),
    });

    Ok(ctx.success().stdout(details.to_string()))
}
