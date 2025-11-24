use crate::job_system::ctx::{Ctx, JobExecutionUpdateExt};
use color_eyre::{
    eyre::{bail, eyre, Context as _},
    Result,
};
use orb_relay_messages::jobs::v1::JobExecutionUpdate;
use serde::{Deserialize, Serialize};
use tracing::info;

use super::reboot::{run_reboot_flow, RebootPlan};

#[derive(Deserialize, Serialize, Debug)]
#[serde(rename_all = "lowercase")]
enum SlotTarget {
    A,
    B,
    Other,
}

#[derive(Deserialize, Serialize, Debug)]
struct SlotSwitchArgs {
    slot: SlotTarget,
}

/// command format: `slot_switch {"slot": "a"|"b"|"other"}`
#[tracing::instrument(skip(ctx))]
pub async fn handler(ctx: Ctx) -> Result<JobExecutionUpdate> {
    info!("Handling slot_switch command for job {}", ctx.execution_id());

    // First, check if there's a pending reboot for this job
    let store_path = &ctx.deps().settings.store_path;
    let reboot_lockfile_path = store_path.join("reboot.lock");

    if let Ok(pending_execution_id) = tokio::fs::read_to_string(&reboot_lockfile_path).await {
        if pending_execution_id == ctx.execution_id() {
            // We just rebooted, complete the job
            info!("Completing slot_switch after reboot for job {}", ctx.execution_id());
            tokio::fs::remove_file(&reboot_lockfile_path).await?;

            ctx.progress()
                .stdout("rebooted")
                .send()
                .await
                .map_err(|e| eyre!("failed to send progress {e:?}"))?;

            return Ok(ctx.success());
        }
    }

    let args: SlotSwitchArgs = ctx.args_json()?;

    let current_slot = get_current_slot(&ctx).await?;
    info!("Current slot: {}", current_slot);

    let target_slot = match args.slot {
        SlotTarget::A => "a",
        SlotTarget::B => "b",
        SlotTarget::Other => {
            if current_slot == "a" {
                "b"
            } else {
                "a"
            }
        }
    };

    info!("Target slot: {}", target_slot);

    if current_slot == target_slot {
        return Ok(ctx.success().stdout(format!(
            "Already on slot {current_slot}, nothing to do"
        )));
    }

    run_reboot_flow(ctx, "slot_switch", |ctx| async move {
        switch_slot(&ctx, target_slot).await?;

        Ok(RebootPlan::with_stdout(format!(
            "Switched from slot {current_slot} to slot {target_slot}, rebooting"
        )))
    })
    .await
}

async fn get_current_slot(ctx: &Ctx) -> Result<String> {
    let output = ctx
        .deps()
        .shell
        .exec(&["orb-slot-ctrl", "-c"])
        .await
        .context("failed to spawn orb-slot-ctrl")?
        .wait_with_output()
        .await
        .context("failed to wait for orb-slot-ctrl")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("orb-slot-ctrl -c failed: {}", stderr);
    }

    let slot = String::from_utf8_lossy(&output.stdout).trim().to_string();

    if slot != "a" && slot != "b" {
        bail!("unexpected slot value from orb-slot-ctrl: '{}'", slot);
    }

    Ok(slot)
}

async fn switch_slot(ctx: &Ctx, target_slot: &str) -> Result<()> {
    info!("Switching to slot {}", target_slot);

    let output = ctx
        .deps()
        .shell
        .exec(&["sudo", "orb-slot-ctrl", "-s", target_slot])
        .await
        .context("failed to spawn orb-slot-ctrl")?
        .wait_with_output()
        .await
        .context("failed to wait for orb-slot-ctrl")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("orb-slot-ctrl -s {} failed: {}", target_slot, stderr);
    }

    Ok(())
}
