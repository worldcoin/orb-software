use color_eyre::eyre::Result;
use std::path::PathBuf;
use tokio::task;
use tracing::{error, info};
use walkdir::WalkDir;

const PERSISTENT: &str = "/usr/persistent";

pub async fn run() -> Result<()> {
    task::spawn_blocking(|| collect(PERSISTENT)).await?
}

fn collect(root: &str) -> Result<()> {
    let root = PathBuf::from(root);
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

    entries.sort_unstable_by(|a, b| b.1.cmp(&a.1));

    for (path, size_bytes) in entries {
        info!("{}: {size_bytes} bytes", path.display());
    }

    Ok(())
}
