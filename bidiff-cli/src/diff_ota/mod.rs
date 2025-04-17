mod diff_plan;
mod execute_plan;
mod ota_dir;
mod patch_claim;

use std::path::Path;

use color_eyre::{eyre::WrapErr as _, Result};
use futures::FutureExt as _;
use orb_update_agent_core::UncheckedClaim;
use tokio::{fs, io::AsyncWriteExt as _};
use tokio_util::sync::CancellationToken;
use tracing::info;

use self::{
    diff_plan::DiffPlan,
    ota_dir::{OtaDir, OutDir},
    patch_claim::patch_claim,
};
use crate::is_empty_dir;

pub const CLAIM_FILE: &str = "claim.json";

/// Preconditions:
/// - `out_dir` should be an empty directory.
pub async fn diff_ota(
    base_dir: &Path,
    top_dir: &Path,
    out_dir: &Path,
    cancel: CancellationToken,
    validate_input_dir_hashes: bool,
) -> Result<()> {
    let _cancel_guard = cancel.clone().drop_guard();
    for d in [base_dir, top_dir, out_dir] {
        assert!(fs::try_exists(d).await?, "{d:?} does not exist");
        assert!(fs::metadata(d).await?.is_dir(), "{d:?} was not a directory");
    }
    assert!(is_empty_dir(out_dir).await?, "{out_dir:?} was not empty");

    let (plan, claim_before_patch) =
        make_plan(base_dir, top_dir, out_dir, validate_input_dir_hashes)
            .await
            .wrap_err("failed to create diffing plan")?;
    info!("created diffing plan: {plan:#?}");

    info!("executing diffing plan");
    let plan_outputs = self::execute_plan::execute_plan(&plan, cancel.child_token())
        .await
        .wrap_err("failed to execute diffing plan")?;

    info!("patching claim");
    let patched_claim = patch_claim(&plan, &plan_outputs, &claim_before_patch);
    let out_claim_path = out_dir.join(CLAIM_FILE);
    let mut out_claim_file = tokio::io::BufWriter::new(
        tokio::fs::File::create_new(&out_claim_path)
            .await
            .expect("infallible: dir is empty and other fns shouldn't create a claim"),
    );
    let serialized_claim = serde_json::to_vec(&patched_claim.0)
        .expect("infallible: the claim should always serialize");
    out_claim_file
        .write_all(&serialized_claim)
        .await
        .wrap_err_with(|| {
            format!("failed to write to output claim file at `{out_claim_path:?}`")
        })?;
    out_claim_file.flush().await?;
    out_claim_file.into_inner().sync_all().await?;

    info!("verifying patched claim");
    let _ = OtaDir::new(out_dir, true)
        .await
        .wrap_err("failed to validate final output dir")?;

    Ok(())
}

async fn make_plan(
    base_dir: &Path,
    top_dir: &Path,
    out_dir: &Path,
    validate_hashes: bool,
) -> Result<(DiffPlan, UncheckedClaim)> {
    info!("validationg that claims match directory contents");
    let base_fut = OtaDir::new(base_dir, validate_hashes).map(|r| {
        r.wrap_err_with(|| {
            format!("failed to validate OTA directory contents at {base_dir:?}")
        })
    });
    let new_fut = OtaDir::new(top_dir, validate_hashes).map(|r| {
        r.wrap_err_with(|| {
            format!("failed to validate OTA directory contents at {top_dir:?}")
        })
    });
    // This takes some time, so we do them concurrently
    let ((old_ota, _old_claim), (new_ota, new_claim)) =
        tokio::try_join!(base_fut, new_fut)?;
    let out_dir = OutDir::new(out_dir);

    info!("computing diffing plan");
    let plan = DiffPlan::new(&old_ota, &new_ota, &out_dir);

    Ok((plan, new_claim))
}

#[cfg(test)]
mod test {
    pub const TEST_FILE: &str = r#"
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣠⣤⣤⣤⣤⣤⣤⣤⣤⣄⡀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⢀⣴⣿⡿⠛⠉⠙⠛⠛⠛⠛⠻⢿⣿⣷⣤⡀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⣼⣿⠋⠀⠀⠀⠀⠀⠀⠀⢀⣀⣀⠈⢻⣿⣿⡄⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⣸⣿⡏⠀⠀⠀⣠⣶⣾⣿⣿⣿⠿⠿⠿⢿⣿⣿⣿⣄⠀⠀
⠀⠀⠀⠀⠀⠀⠀⣿⣿⠁⠀⠀⢰⣿⣿⣯⠁⠀⠀⠀⠀⠀⠀⠀⠈⠙⢿⣷⡄
⠀⠀⣀⣤⣴⣶⣶⣿⡟⠀⠀⠀⢸⣿⣿⣿⣆⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣿⣷
⠀⢰⣿⡟⠋⠉⣹⣿⡇⠀⠀⠀⠘⣿⣿⣿⣿⣷⣦⣤⣤⣤⣶⣶⣶⣶⣿⣿⣿
⠀⢸⣿⡇⠀⠀⣿⣿⡇⠀⠀⠀⠀⠹⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⡿⠃
⠀⣸⣿⡇⠀⠀⣿⣿⡇⠀⠀⠀⠀⠀⠉⠻⠿⣿⣿⣿⣿⡿⠿⠿⠛⢻⣿⡇⠀
⠀⣿⣿⠁⠀⠀⣿⣿⡇⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢸⣿⣧⠀
⠀⣿⣿⠀⠀⠀⣿⣿⡇⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢸⣿⣿⠀
⠀⣿⣿⠀⠀⠀⣿⣿⡇⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢸⣿⣿⠀
⠀⢿⣿⡆⠀⠀⣿⣿⡇⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢸⣿⡇⠀
⠀⠸⣿⣧⡀⠀⣿⣿⡇⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣿⣿⠃⠀
⠀⠀⠛⢿⣿⣿⣿⣿⣇⠀⠀⠀⠀⠀⣰⣿⣿⣷⣶⣶⣶⣶⠶⠀⢠⣿⣿⠀⠀
⠀⠀⠀⠀⠀⠀⠀⣿⣿⠀⠀⠀⠀⠀⣿⣿⡇⠀⣽⣿⡏⠁⠀⠀⢸⣿⡇⠀⠀
⠀⠀⠀⠀⠀⠀⠀⣿⣿⠀⠀⠀⠀⠀⣿⣿⡇⠀⢹⣿⡆⠀⠀⠀⣸⣿⠇⠀⠀
⠀⠀⠀⠀⠀⠀⠀⢿⣿⣦⣄⣀⣠⣴⣿⣿⠁⠀⠈⠻⣿⣿⣿⣿⡿⠏⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠈⠛⠻⠿⠿⠿⠿⠋⠁⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀"#;

    pub const TEST_FILE_SHA256: &str =
        "3854ea6fb341ef8b0a2a9f3840e4581cebcb382d22584b8bfe4f4565a70e95c8";
}
