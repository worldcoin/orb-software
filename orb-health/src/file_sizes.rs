use color_eyre::Result;
use std::path::{Path, PathBuf};
use tracing::{error, info};
use walkdir::WalkDir;

const PERSISTENT: &str = "/usr/persistent";

pub fn run() -> Result<()> {
    let mut entries = collect(Path::new(PERSISTENT))?;
    entries.sort_unstable_by(|a, b| b.1.cmp(&a.1));

    for (path, size_bytes) in entries {
        info!("{}: {}", path.display(), size_bytes);
    }

    Ok(())
}

fn collect(root: &Path) -> Result<Vec<(PathBuf, u64)>> {
    let mut entries = Vec::new();

    for entry in WalkDir::new(root).follow_links(false) {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                error!("failed to read entry: {e}");
                continue;
            }
        };

        if entry.file_type().is_file() {
            match entry.metadata() {
                Ok(meta) => entries.push((entry.into_path(), meta.len())),
                Err(e) => {
                    error!(path = %entry.path().display(), "failed to read metadata: {e}")
                }
            }
        }
    }

    Ok(entries)
}
