use crate::job_system::ctx::{Ctx, JobExecutionUpdateExt};
use color_eyre::{eyre::ContextCompat, Result};
use orb_relay_messages::jobs::v1::JobExecutionUpdate;
use tracing::info;

pub async fn handler(ctx: Ctx) -> Result<JobExecutionUpdate> {
    let file_path = ctx.args().first().wrap_err("no file path argument given")?;
    info!("Reading file: {} for job {}", file_path, ctx.execution_id());
    println!("READING BABYYYY");

    // Execute the file reading logic
    let result = tokio::fs::read(file_path).await?;

    Ok(ctx.success().stdout(String::from_utf8_lossy(&result)))
}
