use crate::job_system::ctx::{Ctx, JobExecutionUpdateExt};
use color_eyre::{
    eyre::{eyre, Context},
    Result,
};
use orb_relay_messages::jobs::v1::JobExecutionUpdate;
use std::path::Path;
use tokio::fs;

const CALIBRATION_PATH: &str = "/usr/persistent/calibration.json";

/// command format: `read_gimbal`
#[tracing::instrument(skip(ctx))]
pub async fn handler(ctx: Ctx) -> Result<JobExecutionUpdate> {
    let calibration_path = Path::new(CALIBRATION_PATH);

    let contents = fs::read_to_string(calibration_path)
        .await
        .with_context(|| {
            eyre!(
                "failed to read calibration from {}",
                calibration_path.display()
            )
        })?;

    // Ensure the payload is valid JSON so callers can rely on it.
    let json: serde_json::Value = serde_json::from_str(&contents)
        .context("calibration file is not valid JSON")?;

    Ok(ctx.success().stdout(json.to_string()))
}
