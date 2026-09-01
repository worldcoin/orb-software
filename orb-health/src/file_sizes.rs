use color_eyre::eyre::{eyre, Result};
use rustix::fs::statvfs;
use std::collections::HashSet;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use tokio::task;
use tracing::{error, info};
use walkdir::WalkDir;

const PERSISTENT: &str = "/usr/persistent";
const STAT_BLOCK_SIZE: u64 = 512;

#[derive(Debug)]
struct FileUsage {
    path: PathBuf,
    logical_bytes: u64,
    allocated_bytes: u64,
    device: u64,
    inode: u64,
}

#[derive(Debug)]
struct Usage {
    total_bytes: u64,
    used_bytes: u64,
    available_bytes: u64,
    allocated_file_bytes: u64,
    overhead_bytes: u64,
    scan_errors: usize,
    files: Vec<FileUsage>,
}

pub async fn run() -> Result<()> {
    task::spawn_blocking(|| collect(PERSISTENT)).await?
}

fn collect(root: &str) -> Result<()> {
    let root = Path::new(root);
    let usage = usage(root)?;

    info!(
        "Filesystem in {}: {} total bytes, {} used bytes, {} available bytes",
        root.display(),
        usage.total_bytes,
        usage.used_bytes,
        usage.available_bytes
    );
    info!(
        "Filesystem overhead in {}: {} bytes ({} used bytes - {} allocated file bytes; {} scan errors)",
        root.display(),
        usage.overhead_bytes,
        usage.used_bytes,
        usage.allocated_file_bytes,
        usage.scan_errors
    );

    for file in usage.files {
        info!(
            "{}: {} logical bytes, {} allocated bytes",
            file.path.display(),
            file.logical_bytes,
            file.allocated_bytes
        );
    }

    Ok(())
}

fn usage(root: &Path) -> Result<Usage> {
    let mut entries = Vec::new();
    let mut scan_errors = 0;

    for entry in WalkDir::new(root)
        .follow_links(false)
        .same_file_system(true)
    {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                error!("failed to read entry: {e}");
                scan_errors += 1;
                continue;
            }
        };

        if entry.file_type().is_file() {
            match entry.metadata() {
                Ok(meta) => entries.push(FileUsage {
                    path: entry.into_path(),
                    logical_bytes: meta.len(),
                    allocated_bytes: meta.blocks().saturating_mul(STAT_BLOCK_SIZE),
                    device: meta.dev(),
                    inode: meta.ino(),
                }),
                Err(e) => {
                    error!(path = %entry.path().display(), "failed to read metadata: {e}");
                    scan_errors += 1;
                }
            }
        }
    }

    entries.sort_unstable_by(|a, b| {
        b.allocated_bytes
            .cmp(&a.allocated_bytes)
            .then_with(|| b.logical_bytes.cmp(&a.logical_bytes))
            .then_with(|| a.path.cmp(&b.path))
    });

    let mut seen_files = HashSet::new();
    let allocated_file_bytes = entries
        .iter()
        .filter(|entry| seen_files.insert((entry.device, entry.inode)))
        .try_fold(0_u64, |total, entry| {
            total
                .checked_add(entry.allocated_bytes)
                .ok_or_else(|| eyre!("allocated file size overflow"))
        })?;

    let stats = statvfs(root)?;
    let fragment_size = if stats.f_frsize == 0 {
        stats.f_bsize
    } else {
        stats.f_frsize
    };
    if fragment_size == 0 {
        return Err(eyre!("filesystem reported a block size of zero"));
    }

    let total_bytes = stats
        .f_blocks
        .checked_mul(fragment_size)
        .ok_or_else(|| eyre!("filesystem total size overflow"))?;
    let used_bytes = stats
        .f_blocks
        .saturating_sub(stats.f_bfree)
        .checked_mul(fragment_size)
        .ok_or_else(|| eyre!("filesystem used size overflow"))?;
    let available_bytes = stats
        .f_bavail
        .checked_mul(fragment_size)
        .ok_or_else(|| eyre!("filesystem available size overflow"))?;
    let overhead_bytes = used_bytes.saturating_sub(allocated_file_bytes);

    if allocated_file_bytes > used_bytes {
        error!(
            allocated_file_bytes,
            used_bytes, "allocated file size exceeds filesystem used size"
        );
    }

    Ok(Usage {
        total_bytes,
        used_bytes,
        available_bytes,
        allocated_file_bytes,
        overhead_bytes,
        scan_errors,
        files: entries,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn reports_logical_and_allocated_file_sizes() {
        let tempdir = tempfile::tempdir().unwrap();
        let path = tempdir.path().join("file");
        fs::write(&path, vec![0_u8; 8192]).unwrap();

        let usage = usage(tempdir.path()).unwrap();

        assert_eq!(usage.files.len(), 1);
        assert_eq!(usage.files[0].path, path);
        assert_eq!(usage.files[0].logical_bytes, 8192);
        assert!(usage.files[0].allocated_bytes >= 8192);
        assert_eq!(usage.allocated_file_bytes, usage.files[0].allocated_bytes);
    }

    #[test]
    fn counts_hardlinked_file_allocation_once() {
        let tempdir = tempfile::tempdir().unwrap();
        let original = tempdir.path().join("original");
        let hardlink = tempdir.path().join("hardlink");
        fs::write(&original, vec![0_u8; 8192]).unwrap();
        fs::hard_link(&original, &hardlink).unwrap();

        let usage = usage(tempdir.path()).unwrap();

        assert_eq!(usage.files.len(), 2);
        assert_eq!(usage.allocated_file_bytes, usage.files[0].allocated_bytes);
    }
}
