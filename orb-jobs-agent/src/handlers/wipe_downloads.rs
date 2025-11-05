use std::path::Path;

use crate::job_system::ctx::{Ctx, JobExecutionUpdateExt};
use color_eyre::{eyre::Context, Result};
use orb_relay_messages::jobs::v1::JobExecutionUpdate;
use tokio::fs;
use tracing::{info, warn};

/// command format: `wipe-downloads`
#[tracing::instrument(skip(ctx))]
pub async fn handler(ctx: Ctx) -> Result<JobExecutionUpdate> {
    info!("Wiping downloads dir for job {}", ctx.execution_id());

    let downloads_path = Path::new("/mnt/scratch/downloads");
    let mut entries = match fs::read_dir(downloads_path).await {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            info!(
                "Downloads dir {} does not exist, nothing to do.",
                downloads_path.display()
            );
            return Ok(ctx
                .success()
                .stdout("Downloads directory does not exist, nothing to delete"));
        }
        Err(e) => {
            return Err(e).context(format!(
                "failed to read {} directory.",
                downloads_path.display()
            ));
        }
    };

    let mut deleted_count = 0;
    let mut failed_count = 0;

    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        let metadata = entry.metadata().await?;

        let result = if metadata.is_dir() {
            fs::remove_dir_all(&path).await
        } else {
            fs::remove_file(&path).await
        };

        match result {
            Ok(_) => {
                deleted_count += 1;
                info!("Deleted {}", path.display());
            }
            Err(e) => {
                failed_count += 1;
                warn!("Failed to delete {}: {}", path.display(), e);
            }
        }

        if ctx.is_cancelled() {
            return Ok(ctx.cancelled().stdout(format!(
                "Deletion cancelled. Deleted {deleted_count}, Failed: {failed_count}",
            )));
        }
    }

    let result_message = format!("Deleted {deleted_count}, Failed {failed_count}");
    info!(result_message);

    if failed_count > 0 {
        Ok(ctx.failure().stdout(result_message))
    } else {
        Ok(ctx.success().stdout(result_message))
    }
}
