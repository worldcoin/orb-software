use std::path::Path;

use color_eyre::{eyre::WrapErr as _, Result};
use tokio::fs;
use tokio_util::sync::CancellationToken;
use tracing::info;

use crate::diff_plan::DiffPlan;

const CLAIM_FILENAME: &str = "claim.json";

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

    let plan = make_plan(base_dir, top_dir)
        .await
        .wrap_err("failed to create diffing plan")?;
    info!("created diffing plan: {plan:#?}");

    execute_plan(&plan)
        .await
        .wrap_err("failed to execute diffing plan")
}

async fn execute_plan(_plan: &DiffPlan) -> Result<()> {
    todo!("plan execution")
}

async fn make_plan(base_dir: &Path, top_dir: &Path) -> Result<DiffPlan> {
    let old_claim_path = base_dir.join(CLAIM_FILENAME);
    let new_claim_path = top_dir.join(CLAIM_FILENAME);

    let plan = crate::diff_plan::DiffPlan::new(&old_claim_path, &new_claim_path)
        .await
        .wrap_err("failed to create diffing plan from claim paths")?;

    Ok(plan)
}
