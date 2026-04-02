use super::service_control;
use crate::job_system::ctx::{Ctx, JobExecutionUpdateExt};
use color_eyre::{
    eyre::{ensure, Context},
    Result,
};
use orb_relay_messages::jobs::v1::JobExecutionUpdate;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::fs;
use tracing::info;

#[derive(Debug, Deserialize, Serialize, PartialEq)]
struct RgbFocusCalibration {
    bias: f64,
    calibrated: bool,
    samples: u64,
}

#[derive(Debug, Serialize)]
struct ResetRgbFocusCalibrationResponse {
    recreated: bool,
    calibration: RgbFocusCalibration,
}

/// command format: `reset_rgb_focus_calibration <bias>`
#[tracing::instrument(skip(ctx))]
pub async fn handler(ctx: Ctx) -> Result<JobExecutionUpdate> {
    let args = ctx.args();
    ensure!(args.len() == 1, "Expected 1 argument, got {}", args.len());

    let bias = args[0]
        .parse::<f64>()
        .with_context(|| format!("failed to parse bias as number: {}", args[0]))?;

    let current_path = &ctx.deps().settings.rgb_focus_calibration_file_path;

    info!(
        bias,
        current_path = %current_path.display(),
        "resetting RGB focus calibration bias"
    );

    let (mut calibration, recreated) = load_calibration(current_path, bias).await;

    calibration.bias = bias;
    write_calibration(current_path, &calibration).await?;
    service_control::restart_service(
        &ctx,
        "worldcoin-core.service",
        "RGB focus calibration",
    )
    .await?;

    let response = ResetRgbFocusCalibrationResponse {
        recreated,
        calibration,
    };
    let response_json = serde_json::to_string_pretty(&response)
        .context("failed to serialize RGB focus calibration response")?;

    Ok(ctx.success().stdout(response_json))
}

async fn load_calibration(
    current_path: &Path,
    bias: f64,
) -> (RgbFocusCalibration, bool) {
    match read_and_validate_calibration(current_path).await {
        Ok(calibration) => (calibration, false),
        Err(_) => (RgbFocusCalibration::recreated(bias), true),
    }
}

async fn read_and_validate_calibration(path: &Path) -> Result<RgbFocusCalibration> {
    let contents = fs::read_to_string(path).await.with_context(|| {
        format!(
            "failed to read RGB focus calibration file at {}",
            path.display()
        )
    })?;

    serde_json::from_str(&contents).with_context(|| {
        format!(
            "failed to parse RGB focus calibration file at {}",
            path.display()
        )
    })
}

async fn write_calibration(
    path: &Path,
    calibration: &RgbFocusCalibration,
) -> Result<()> {
    let json = serde_json::to_string_pretty(calibration)
        .context("failed to serialize RGB focus calibration JSON")?;

    fs::write(path, format!("{json}\n"))
        .await
        .with_context(|| {
            format!(
                "failed to write RGB focus calibration file at {}",
                path.display()
            )
        })?;

    Ok(())
}
impl RgbFocusCalibration {
    fn recreated(bias: f64) -> Self {
        Self {
            bias,
            calibrated: false,
            samples: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calibration_schema_accepts_expected_shape() {
        let calibration: RgbFocusCalibration = serde_json::from_str(
            r#"{"bias":146.01668037487582,"calibrated":false,"samples":34}"#,
        )
        .unwrap();

        assert_eq!(calibration.bias, 146.01668037487582);
        assert!(!calibration.calibrated);
        assert_eq!(calibration.samples, 34);
    }

    #[test]
    fn calibration_schema_rejects_missing_fields() {
        let result = serde_json::from_str::<RgbFocusCalibration>(
            r#"{"bias":146.01668037487582,"calibrated":false}"#,
        );

        assert!(result.is_err());
    }
}
