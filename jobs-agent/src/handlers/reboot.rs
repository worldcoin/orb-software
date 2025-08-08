use std::path::Path;

use crate::job_system::ctx::Ctx;
use color_eyre::{eyre::eyre, Result};
use orb_relay_messages::jobs::v1::{JobExecutionStatus, JobExecutionUpdate};
use tokio::{fs, io};
use tracing::info;

/// command format: `reboot`
#[tracing::instrument]
pub async fn handler(ctx: Ctx) -> Result<JobExecutionUpdate> {
    info!("Handling reboot command for job {}", ctx.execution_id());
    let store_path = &ctx.deps().settings.store_path;

    let reboot_status = RebootStatus::from_lockfile(store_path).await?;

    match reboot_status {
        RebootStatus::Pending(job_execution_id)
            if job_execution_id == ctx.execution_id() =>
        {
            info!("Orb rebooted due to job execution {:?}", ctx.execution_id());
            RebootStatus::remove_pending_lockfile(store_path).await?;

            ctx.progress()
                .stdout("rebooted")
                .send()
                .await
                .map_err(|e| eyre!("failed to send progress {e:?}"))?;
        }

        // we are free to do a reboot
        _ => {
            RebootStatus::write_pending_lockfile(store_path, ctx.execution_id())
                .await?;

            ctx.progress()
                .stdout("rebooting")
                .send()
                .await
                .map_err(|e| eyre!("failed to send progress {e:?}"))?;

            // we need a better way to reboot, regular reboot MIGHT decrease retry counter
            // (lookin into it rn)
            // so not the best idea to reboot through logind and dbus
            ctx.deps().shell.exec(&["reboot"]).await?;

            return Ok(ctx.status(JobExecutionStatus::InProgress));
        }
    }

    Ok(ctx.success())
}

enum RebootStatus {
    /// There is a pending reboot. If this file exists the reboot was most likely executed.
    Pending(String),
    /// We are free to perform a reboot
    Free,
}

impl RebootStatus {
    const FILENAME: &str = "reboot.lock";

    async fn from_lockfile(store: impl AsRef<Path>) -> Result<Self> {
        match fs::read_to_string(store.as_ref().join(Self::FILENAME)).await {
            Ok(s) => Ok(RebootStatus::Pending(s)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(RebootStatus::Free),
            Err(e) => Err(e.into()),
        }
    }

    async fn write_pending_lockfile(
        store: impl AsRef<Path>,
        job_execution_id: impl Into<String>,
    ) -> Result<()> {
        fs::write(store.as_ref().join(Self::FILENAME), job_execution_id.into()).await?;
        Ok(())
    }

    async fn remove_pending_lockfile(store: impl AsRef<Path>) -> Result<()> {
        fs::remove_file(store.as_ref().join(Self::FILENAME)).await?;
        Ok(())
    }
}
