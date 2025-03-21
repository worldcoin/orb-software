use color_eyre::Result;
use std::path::Path;

/// Preconditions:
/// - `out_dir` should be an empty directory.
pub fn diff_ota(_base_dir: &Path, _top_dir: &Path, out_dir: &Path) -> Result<()> {
    assert!(out_dir.is_dir(), "only directories should be provided");
    assert!(out_dir.exists(), "out_dir should exist");

    todo!()
}
