mod diff_plan;
mod execute_plan;
mod ota_dir;
mod patch_claim;

use std::path::Path;

use color_eyre::{eyre::WrapErr as _, Result};
use orb_update_agent_core::UncheckedClaim;
use tokio::{fs, io::AsyncWriteExt as _};
use tokio_util::sync::CancellationToken;
use tracing::info;

use self::{
    diff_plan::{DiffPlan, CLAIM_FILE},
    ota_dir::{OtaDir, OutDir},
    patch_claim::patch_claim,
};
use crate::is_empty_dir;

/// Preconditions:
/// - `out_dir` should be an empty directory.
pub async fn diff_ota(
    base_dir: &Path,
    top_dir: &Path,
    out_dir: &Path,
    cancel: CancellationToken,
) -> Result<()> {
    let _cancel_guard = cancel.clone().drop_guard();
    for d in [base_dir, top_dir, out_dir] {
        assert!(fs::try_exists(d).await?, "{d:?} does not exist");
        assert!(fs::metadata(d).await?.is_dir(), "{d:?} was not a directory");
    }
    assert!(is_empty_dir(out_dir).await?, "{out_dir:?} was not empty");

    let (plan, claim_before_patch) = make_plan(base_dir, top_dir, out_dir)
        .await
        .wrap_err("failed to create diffing plan")?;
    info!("created diffing plan: {plan:#?}");

    let plan_outputs = self::execute_plan::execute_plan(&plan)
        .await
        .wrap_err("failed to execute diffing plan")?;

    let patched_claim = patch_claim(&plan, &plan_outputs, &claim_before_patch);

    let out_claim_path = out_dir.join(CLAIM_FILE);
    let mut out_claim_file = tokio::fs::File::create_new(&out_claim_path)
        .await
        .expect("infallible: dir is empty and other fns shouldn't create a claim");
    let serialized_claim = serde_json::to_vec(&patched_claim.0)
        .expect("infallible: the claim should always serialize");
    out_claim_file
        .write_all(&serialized_claim)
        .await
        .wrap_err_with(|| {
            format!("failed to write to output claim file at `{out_claim_path:?}`")
        })?;

    Ok(())
}

async fn make_plan(
    base_dir: &Path,
    top_dir: &Path,
    out_dir: &Path,
) -> Result<(DiffPlan, UncheckedClaim)> {
    let (old_ota, _old_claim) = OtaDir::new(base_dir).await.wrap_err_with(|| {
        format!("failed to validate OTA directory contents at {base_dir:?}")
    })?;
    let (new_ota, new_claim) = OtaDir::new(top_dir).await.wrap_err_with(|| {
        format!("failed to validate OTA directory contents at {top_dir:?}")
    })?;
    let out_dir = OutDir::new(out_dir);

    let plan = DiffPlan::new(&old_ota, &new_ota, &out_dir);

    Ok((plan, new_claim))
}
