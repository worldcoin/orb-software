use crate::job_system::ctx::{Ctx, JobExecutionUpdateExt};
use chrono::Utc;
use color_eyre::{
    eyre::{bail, Context, ContextCompat},
    Result,
};
use orb_info::orb_os_release::OrbOsPlatform;
use orb_relay_messages::jobs::v1::{JobExecutionStatus, JobExecutionUpdate};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};
use tokio::{
    fs::{self, OpenOptions},
    io::AsyncWriteExt,
};
use tracing::info;

const DESIRED_PHI_OFFSET: f64 = 0.46;
const DESIRED_THETA_OFFSET: f64 = 0.12;

#[derive(Debug, Serialize, Deserialize)]
struct CalibrationData {
    #[serde(default)]
    phi_offset_degrees: Option<f64>,
    #[serde(default)]
    theta_offset_degrees: Option<f64>,
    /// Preserves all other fields from the calibration file that we don't explicitly handle
    #[serde(flatten)]
    other: HashMap<String, Value>,
}

#[derive(Debug, Serialize)]
struct ResetGimbalResponse {
    backup: String,
    calibration: CalibrationData,
}

#[tracing::instrument(skip(ctx))]
pub async fn handler(ctx: Ctx) -> Result<JobExecutionUpdate> {
    let os_release_path = &ctx.deps().settings.os_release_path;
    let os_release_contents = fs::read_to_string(os_release_path)
        .await
        .context("failed to read Orb OS release file")?;
    let os_release = orb_info::orb_os_release::OrbOsRelease::parse(os_release_contents)
        .context("failed to parse Orb OS release information")?;

    if os_release.orb_os_platform_type != OrbOsPlatform::Pearl {
        return Ok(ctx
            .status(JobExecutionStatus::FailedUnsupported)
            .stderr("reset_gimbal is only supported on Pearl devices"));
    }

    let calibration_path = &ctx.deps().settings.calibration_file_path;

    fs::metadata(calibration_path).await.with_context(|| {
        format!("calibration file not found: {}", calibration_path.display())
    })?;

    let backup_path = create_backup(calibration_path).await?;
    info!(?backup_path, "created calibration backup");

    let updated_calibration = update_calibration_file(calibration_path).await?;

    let response = ResetGimbalResponse {
        backup: backup_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_string(),
        calibration: updated_calibration,
    };

    let response_json = serde_json::to_string(&response)
        .context("failed to serialize reset_gimbal response")?;

    Ok(ctx.success().stdout(response_json))
}

async fn create_backup(calibration_path: &Path) -> Result<PathBuf> {
    let parent = calibration_path
        .parent()
        .context("calibration file must reside in a directory")?;

    let timestamp = Utc::now().format("%Y-%m-%d_%H-%M");
    let filename = calibration_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("calibration.json");
    let backup_name = format!("{filename}.{timestamp}.bak");

    let backup_path = parent.join(backup_name);

    fs::copy(calibration_path, &backup_path)
        .await
        .with_context(|| {
            format!(
                "failed to create calibration backup at {}",
                backup_path.display()
            )
        })?;

    Ok(backup_path)
}

async fn update_calibration_file(calibration_path: &Path) -> Result<CalibrationData> {
    let contents = fs::read_to_string(calibration_path)
        .await
        .with_context(|| {
            format!(
                "failed to read calibration file at {}",
                calibration_path.display()
            )
        })?;

    let mut calibration: CalibrationData = serde_json::from_str(&contents)
        .context("failed to parse calibration.json as JSON")?;

    // Verify required fields exist
    if calibration.phi_offset_degrees.is_none() {
        bail!("missing phi_offset_degrees in calibration.json");
    }
    if calibration.theta_offset_degrees.is_none() {
        bail!("missing theta_offset_degrees in calibration.json");
    }

    // Update the values
    calibration.phi_offset_degrees = Some(DESIRED_PHI_OFFSET);
    calibration.theta_offset_degrees = Some(DESIRED_THETA_OFFSET);

    let serialized = serde_json::to_string_pretty(&calibration)
        .context("failed to serialize calibration JSON")?;

    let mut file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(calibration_path)
        .await
        .with_context(|| {
            format!(
                "failed to open calibration file for writing at {}",
                calibration_path.display()
            )
        })?;

    file.write_all(serialized.as_bytes())
        .await
        .context("failed to write updated calibration file")?;
    file.write_all(b"\n")
        .await
        .context("failed to append trailing newline to calibration file")?;
    file.flush()
        .await
        .context("failed to flush updated calibration file")?;

    Ok(calibration)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calibration_data_serialization() {
        let mut extra = HashMap::new();
        extra.insert(
            "extra_field".to_string(),
            Value::String("extra_value".to_string()),
        );
        extra.insert("nested".to_string(), serde_json::json!({"key": "value"}));

        let calibration = CalibrationData {
            phi_offset_degrees: Some(0.46),
            theta_offset_degrees: Some(0.12),
            other: extra,
        };

        // Serialize to JSON
        let json = serde_json::to_string(&calibration).unwrap();

        // Verify it contains our fields
        assert!(json.contains("phi_offset_degrees"));
        assert!(json.contains("theta_offset_degrees"));
        assert!(json.contains("extra_field"));

        // Deserialize back
        let deserialized: CalibrationData = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.phi_offset_degrees, Some(0.46));
        assert_eq!(deserialized.theta_offset_degrees, Some(0.12));
        assert!(deserialized.other.contains_key("extra_field"));
    }

    #[test]
    fn test_reset_gimbal_response_serialization() {
        let mut extra = HashMap::new();
        extra.insert(
            "extra_field".to_string(),
            Value::String("extra_value".to_string()),
        );

        let response = ResetGimbalResponse {
            backup: "calibration.json.2025-01-01_12-00.bak".to_string(),
            calibration: CalibrationData {
                phi_offset_degrees: Some(0.46),
                theta_offset_degrees: Some(0.12),
                other: extra,
            },
        };

        // Serialize to JSON
        let json = serde_json::to_string(&response).unwrap();

        // Verify it contains expected fields
        assert!(json.contains("backup"));
        assert!(json.contains("calibration"));
        assert!(json.contains("phi_offset_degrees"));
        assert!(json.contains("theta_offset_degrees"));
    }
}
