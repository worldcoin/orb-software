use std::path::Path;

use color_eyre::{eyre::WrapErr as _, Result};
use tokio::fs;
use tokio_util::sync::CancellationToken;
use tracing::info;

use crate::diff_plan::{patch_claim, DiffPlan, OtaDir, OutDir, PatchedClaim};

/// Preconditions:
/// - `out_dir` should be an empty directory.
pub async fn diff_ota(
    base_dir: &Path,
    top_dir: &Path,
    out_dir: &Path,
    cancel: CancellationToken,
) -> Result<()> {
    let _cancel_guard = cancel.drop_guard();
    for d in [base_dir, top_dir, out_dir] {
        assert!(
            fs::try_exists(d).await.unwrap_or(false),
            "{d:?} does not exist"
        );
        assert!(fs::metadata(d).await?.is_dir(), "{d:?} was not a directory");
    }

    let (plan, patched_claim) = make_plan(base_dir, top_dir, out_dir)
        .await
        .wrap_err("failed to create diffing plan")?;
    info!("created diffing plan: {plan:#?}");
    info!("created patched claim: {patched_claim:#?}");

    execute_plan(plan, patched_claim)
        .await
        .wrap_err("failed to execute diffing plan")
}

async fn execute_plan(_plan: DiffPlan, _patched_claim: PatchedClaim) -> Result<()> {
    todo!("plan execution")
}

async fn make_plan(
    base_dir: &Path,
    top_dir: &Path,
    out_dir: &Path,
) -> Result<(DiffPlan, PatchedClaim)> {
    let (old_ota, _old_claim) = OtaDir::new(base_dir).await.wrap_err_with(|| {
        format!("failed to validate OTA directory contents at {base_dir:?}")
    })?;
    let (new_ota, new_claim) = OtaDir::new(top_dir).await.wrap_err_with(|| {
        format!("failed to validate OTA directory contents at {top_dir:?}")
    })?;
    let out_dir = OutDir::new(out_dir);

    let plan = DiffPlan::new(&old_ota, &new_ota, &out_dir);
    let patched_claim = patch_claim(&plan, &new_claim);

    Ok((plan, patched_claim))
}
