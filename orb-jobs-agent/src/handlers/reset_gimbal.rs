use super::reboot::{self, RebootPlan};
use crate::job_system::ctx::{Ctx, JobExecutionUpdateExt};
use chrono::{Duration, Utc};
use color_eyre::{
    eyre::{bail, eyre, Context, ContextCompat},
    Result,
};
use orb_info::orb_os_release::{OrbOsPlatform, OrbOsRelease};
use orb_relay_messages::jobs::v1::{JobExecutionStatus, JobExecutionUpdate};
use serde_json::Value;
use std::path::{Path, PathBuf};
use tokio::{
    fs::{self, OpenOptions},
    io::AsyncWriteExt,
};
use tracing::info;
use zbus::{proxy, Connection};

const PERSISTENT_DIR: &str = "/usr/persistent";
const CALIBRATION_FILENAME: &str = "calibration.json";
const DESIRED_PHI_OFFSET: f64 = 0.46;
const DESIRED_THETA_OFFSET: f64 = 0.12;
const SHUTDOWN_KIND: &str = "reboot";

#[tracing::instrument(skip(ctx))]
pub async fn handler(ctx: Ctx) -> Result<JobExecutionUpdate> {
    let os_release = OrbOsRelease::read()
        .await
        .context("failed to read Orb OS release information")?;

    if os_release.orb_os_platform_type != OrbOsPlatform::Pearl {
        return Ok(ctx
            .status(JobExecutionStatus::FailedUnsupported)
            .stderr("reset_gimbal is only supported on Pearl devices"));
    }

    reboot::run_reboot_flow(ctx, "reset_gimbal", |ctx| async move {
        let calibration_path = Path::new(PERSISTENT_DIR).join(CALIBRATION_FILENAME);

        fs::metadata(&calibration_path).await.with_context(|| {
            format!("calibration file not found: {}", calibration_path.display())
        })?;

        let backup_path = create_backup(&calibration_path).await?;
        info!(?backup_path, "created calibration backup");

        let updated_calibration = update_calibration_file(&calibration_path).await?;

        let scheduled_micros = compute_reboot_deadline()?;
        schedule_reboot(scheduled_micros).await?;
        info!(scheduled_micros, "scheduled reboot after reset");

        let response = serde_json::json!({
            "backup": backup_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default(),
            "calibration": updated_calibration,
            "scheduled_shutdown_usec": scheduled_micros,
        });

        ctx.progress()
            .stdout(response.to_string())
            .send()
            .await
            .map_err(|e| eyre!("failed to send reset_gimbal progress update {e:?}"))?;

        Ok(RebootPlan::with_stdout("rebooting\n"))
    })
    .await
}

async fn create_backup(calibration_path: &Path) -> Result<PathBuf> {
    let parent = calibration_path
        .parent()
        .context("calibration file must reside in a directory")?;

    let timestamp = Utc::now().format("%Y-%m-%d_%H-%M");
    let backup_name = format!("{}.{}.bak", CALIBRATION_FILENAME, timestamp);

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

async fn update_calibration_file(calibration_path: &Path) -> Result<Value> {
    let contents = fs::read_to_string(calibration_path)
        .await
        .with_context(|| {
            format!(
                "failed to read calibration file at {}",
                calibration_path.display()
            )
        })?;

    let mut calibration: Value = serde_json::from_str(&contents)
        .context("failed to parse calibration.json as JSON")?;

    let phi_updated =
        set_numeric_value(&mut calibration, "phi_offset_degrees", DESIRED_PHI_OFFSET);
    let theta_updated = set_numeric_value(
        &mut calibration,
        "theta_offset_degrees",
        DESIRED_THETA_OFFSET,
    );

    if !phi_updated || !theta_updated {
        bail!(
            "missing expected keys phi_offset_degrees/theta_offset_degrees in calibration.json"
        );
    }

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

fn compute_reboot_deadline() -> Result<u64> {
    let deadline = Utc::now() + Duration::seconds(1);
    deadline
        .timestamp_micros()
        .try_into()
        .context("scheduled shutdown time is before UNIX epoch")
}

async fn schedule_reboot(micros: u64) -> Result<()> {
    let connection = Connection::session()
        .await
        .context("failed to connect to session D-Bus")?;

    let proxy = SupervisorManagerProxy::new(&connection)
        .await
        .context("failed to build supervisor manager proxy")?;

    proxy
        .schedule_shutdown(SHUTDOWN_KIND, micros)
        .await
        .context("failed to schedule reboot via supervisor")?;

    Ok(())
}

#[proxy(
    interface = "org.worldcoin.OrbSupervisor1.Manager",
    default_service = "org.worldcoin.OrbSupervisor1",
    default_path = "/org/worldcoin/OrbSupervisor1/Manager"
)]
trait SupervisorManager {
    #[zbus(name = "ScheduleShutdown")]
    fn schedule_shutdown(&self, kind: &str, when: u64) -> zbus::Result<()>;
}

fn set_numeric_value(tree: &mut Value, key: &str, value: f64) -> bool {
    match tree {
        Value::Object(map) => {
            let mut updated = false;

            if let Some(entry) = map.get_mut(key) {
                *entry = Value::from(value);
                updated = true;
            }

            for child in map.values_mut() {
                if set_numeric_value(child, key, value) {
                    updated = true;
                }
            }

            updated
        }
        Value::Array(items) => {
            let mut updated = false;
            for child in items {
                if set_numeric_value(child, key, value) {
                    updated = true;
                }
            }
            updated
        }
        _ => false,
    }
}
