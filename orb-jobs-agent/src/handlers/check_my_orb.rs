use crate::job_system::ctx::{Ctx, JobExecutionUpdateExt};
use color_eyre::{eyre::Context, Result};
use orb_relay_messages::jobs::v1::JobExecutionUpdate;

/// command format: `check_my_orb [--json]`
/// 
/// examples:
/// - `check_my_orb` - returns output in default format
/// - `check_my_orb --json` - returns output in JSON format
#[tracing::instrument]
pub async fn handler(ctx: Ctx) -> Result<JobExecutionUpdate> {
    // Check if --json flag is provided in arguments
    let use_json = ctx.args().iter().any(|arg| arg == "--json");
    
    // Build command with optional --json flag
    let mut cmd = vec!["check-my-orb"];
    if use_json {
        cmd.push("--json");
    }
    
    let output = ctx
        .deps()
        .shell
        .exec(&cmd)
        .await
        .wrap_err("failed to spawn check_my_orb")?
        .wait_with_output()
        .await
        .wrap_err("failed to get output for check-my-orb")?;

    let output = String::from_utf8_lossy(&output.stdout).to_string();

    Ok(ctx.success().stdout(output))
}
