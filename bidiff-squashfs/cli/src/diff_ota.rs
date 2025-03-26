use color_eyre::Result;
use std::path::Path;

use tokio_util::sync::CancellationToken;

/// Preconditions:
/// - `out_dir` should be an empty directory.
pub fn diff_ota(
    _base_dir: &Path,
    _top_dir: &Path,
    out_dir: &Path,
    cancel: CancellationToken,
) -> Result<()> {
    let _cancel_guard = cancel.drop_guard();
    assert!(out_dir.is_dir(), "only directories should be provided");
    assert!(out_dir.exists(), "out_dir should exist");

    todo!()
}
