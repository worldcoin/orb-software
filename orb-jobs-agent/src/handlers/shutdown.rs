use crate::job_system::ctx::Ctx;
use color_eyre::Result;
use futures::TryFutureExt;
use orb_relay_messages::jobs::v1::JobExecutionUpdate;
use std::{sync::Arc, time::Duration};
use tokio::{
    task::{self},
    time,
};
use tracing::{error, info};

/// command format: `shutdown`
#[tracing::instrument(skip(ctx))]
pub async fn handler(ctx: Ctx) -> Result<JobExecutionUpdate> {
    let execution_id = ctx.execution_id().to_owned();
    info!(execution_id, "Shutting down orb");

    let shell = Arc::clone(&ctx.deps().shell);

    task::spawn(async move {
        time::sleep(Duration::from_secs(4)).await;
        let result = shell.exec(&["shutdown", "now"]).and_then(async |child| {
            child.wait_with_output().await?;
            Ok(())
        });

        if let Err(e) = result.await {
            error!(execution_id, "failed to execute shutdown, err {e}");
        }
    });

    Ok(ctx.success())
}
