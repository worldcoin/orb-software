use crate::job_system::ctx::{Ctx, JobExecutionUpdateExt};
use color_eyre::{
    eyre::{bail, ensure},
    Result,
};
use orb_relay_messages::jobs::v1::JobExecutionUpdate;
use tracing::info;

const CORE_SERVICE: &str = "worldcoin-core.service";
const STOP_CORE_CMD: [&str; 3] = ["systemctl", "stop", CORE_SERVICE];
const START_CORE_CMD: [&str; 3] = ["systemctl", "start", CORE_SERVICE];
const STOP_CORE_STEP: &str = "stopping worldcoin-core.service";
const START_CORE_STEP: &str = "starting worldcoin-core.service";
const CALIBRATION_STEP: &str = "running thermal camera calibration";
const CALIBRATION_CMD: [&str; 5] = [
    "/usr/bin/env",
    "SEEKTHERMAL_ROOT=/usr/persistent",
    "/usr/bin/orb-thermal-cam-ctrl",
    "calibration",
    "fsc",
];

/// command format: `thermal_cam_recalibration`
#[tracing::instrument(skip(ctx))]
pub async fn handler(ctx: Ctx) -> Result<JobExecutionUpdate> {
    ensure!(
        ctx.args().is_empty(),
        "Expected no arguments, got {}",
        ctx.args().len()
    );

    info!(
        "Running thermal camera recalibration for job {}",
        ctx.execution_id()
    );

    if let Err(stop_err) = run_command(&ctx, &STOP_CORE_CMD, STOP_CORE_STEP).await {
        let start_result = run_command(&ctx, &START_CORE_CMD, START_CORE_STEP).await;

        if let Err(start_err) = start_result {
            bail!(
                "{}; additionally failed to start worldcoin-core.service: {}",
                stop_err,
                start_err
            );
        }

        return Err(stop_err);
    }

    let calibration_result =
        run_command(&ctx, &CALIBRATION_CMD, CALIBRATION_STEP).await;

    let start_result = run_command(&ctx, &START_CORE_CMD, START_CORE_STEP).await;

    match (calibration_result, start_result) {
        (Ok(()), Ok(())) => Ok(ctx.success().stdout(
            "Thermal camera recalibration completed and worldcoin-core.service restarted",
        )),
        (Err(calibration_err), Ok(())) => Err(calibration_err),
        (Ok(()), Err(start_err)) => Err(start_err),
        (Err(calibration_err), Err(start_err)) => bail!(
            "{}; additionally failed to restart worldcoin-core.service: {}",
            calibration_err,
            start_err
        ),
    }
}

async fn run_command(ctx: &Ctx, cmd: &[&str], step_name: &str) -> Result<()> {
    let child = ctx.deps().shell.exec(cmd).await?;
    let output = child.wait_with_output().await?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let status = output.status.code().map_or_else(
        || "terminated by signal".to_string(),
        |code| format!("exit status {code}"),
    );

    if stderr.is_empty() && stdout.is_empty() {
        bail!("{step_name} failed with {status}");
    }

    if stderr.is_empty() {
        bail!("{step_name} failed with {status}: stdout: {stdout}");
    }

    bail!("{step_name} failed with {status}: stderr: {stderr}");
}
