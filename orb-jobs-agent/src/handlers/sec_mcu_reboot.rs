use crate::job_system::ctx::{Ctx, JobExecutionUpdateExt};
use color_eyre::{
    eyre::{bail, Context},
    Result,
};
use orb_relay_messages::jobs::v1::JobExecutionUpdate;
use tracing::info;

/// command format: `sec_mcu_reboot`
#[tracing::instrument(skip(ctx))]
pub async fn handler(ctx: Ctx) -> Result<JobExecutionUpdate> {
    info!("Rebooting security MCU for job {}", ctx.execution_id());

    ctx.progress()
        .stdout("Rebooting security MCU...\n")
        .send()
        .await
        .map_err(|e| color_eyre::eyre::eyre!("failed to send progress {e:?}"))?;

    let out = ctx
        .deps()
        .shell
        .exec(&["orb-mcu-util", "--can-fd", "reboot", "security"])
        .await?
        .wait_with_output()
        .await?;

    if !out.status.success() {
        let stderr = String::from_utf8(out.stderr)
            .wrap_err("failed to parse orb-mcu-util stderr")?;
        bail!("`orb-mcu-util --can-fd reboot security` failed: {}", stderr);
    }

    let stdout = String::from_utf8(out.stdout)
        .wrap_err("failed to parse orb-mcu-util stdout")?;

    Ok(ctx.success().stdout(stdout))
}
