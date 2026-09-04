use color_eyre::eyre::{ensure, Context, Result};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use tokio::{fs, io};

pub(super) async fn allocated_size(root: &Path) -> Result<u64> {
    let metadata = fs::symlink_metadata(root).await.wrap_err_with(|| {
        format!("failed to inspect NM state directory {}", root.display())
    })?;
    ensure!(
        metadata.is_dir(),
        "NM state path is not a directory: {}",
        root.display()
    );

    let mut allocated_bytes = metadata.blocks() * 512;
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        let mut entries = match fs::read_dir(&path).await {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound && path != root => continue,
            Err(e) => {
                return Err(e).wrap_err_with(|| {
                    format!("failed to read NM state directory {}", path.display())
                });
            }
        };

        while let Some(entry) = entries.next_entry().await.wrap_err_with(|| {
            format!("failed to enumerate NM state directory {}", path.display())
        })? {
            let metadata = match entry.metadata().await {
                Ok(metadata) => metadata,
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                Err(e) => {
                    return Err(e).wrap_err_with(|| {
                        format!(
                            "failed to inspect NM state entry {}",
                            entry.path().display()
                        )
                    });
                }
            };

            if metadata.is_file() || metadata.is_dir() {
                allocated_bytes += metadata.blocks() * 512;
            }
            if metadata.is_dir() {
                pending.push(entry.path());
            }
        }
    }

    Ok(allocated_bytes)
}

pub(super) async fn remove_disposable_files(varlib: &Path) -> Result<()> {
    let metadata = match fs::symlink_metadata(varlib).await {
        Ok(metadata) => metadata,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => {
            return Err(e).wrap_err_with(|| {
                format!("failed to inspect NM lease directory {}", varlib.display())
            });
        }
    };
    ensure!(
        metadata.is_dir(),
        "NM lease path is not a directory: {}",
        varlib.display()
    );

    let mut entries = fs::read_dir(varlib).await.wrap_err_with(|| {
        format!("failed to read NM lease directory {}", varlib.display())
    })?;
    let mut to_delete = Vec::new();
    while let Some(entry) = entries.next_entry().await.wrap_err_with(|| {
        format!(
            "failed to enumerate NM lease directory {}",
            varlib.display()
        )
    })? {
        let path = entry.path();
        if entry.file_name() != "seen-bssids"
            && !path.extension().is_some_and(|ext| ext == "lease")
        {
            continue;
        }
        let file_type = match entry.file_type().await {
            Ok(file_type) => file_type,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => {
                return Err(e).wrap_err_with(|| {
                    format!("failed to inspect NM disposable file {}", path.display())
                });
            }
        };
        if file_type.is_file() {
            to_delete.push(path);
        }
    }

    remove_files(to_delete).await
}

async fn remove_files(paths: Vec<PathBuf>) -> Result<()> {
    let mut first_error = None;
    let mut failed = 0;
    let mut removed = 0;
    for path in paths {
        match fs::remove_file(&path).await {
            Ok(()) => removed += 1,
            Err(e) if e.kind() == io::ErrorKind::NotFound => (),
            Err(e) => {
                failed += 1;
                first_error.get_or_insert_with(|| {
                    color_eyre::Report::from(e).wrap_err(format!(
                        "failed to remove NM state file {}",
                        path.display()
                    ))
                });
            }
        }
    }

    if let Some(error) = first_error {
        return Err(error.wrap_err(format!(
            "partial NM state cleanup: {removed} files removed, {failed} deletions failed"
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_tempfile::TempDir;

    #[tokio::test]
    async fn it_counts_allocated_space_without_following_symlinks() {
        // Arrange
        let dir = TempDir::new().await.unwrap();
        let state = dir.join("state");
        fs::create_dir(&state).await.unwrap();
        fs::write(state.join("small.lease"), [1; 4097])
            .await
            .unwrap();
        // Act
        let allocated = allocated_size(&state).await.unwrap();

        // Assert
        assert!(allocated > 4097);

        // Arrange
        let outside = dir.join("outside");
        fs::create_dir(&outside).await.unwrap();
        fs::write(outside.join("large"), vec![1; 2 * 1024 * 1024])
            .await
            .unwrap();
        fs::symlink(&outside, state.join("linked-directory"))
            .await
            .unwrap();
        fs::symlink(outside.join("large"), state.join("linked-file"))
            .await
            .unwrap();

        // Act
        let allocated = allocated_size(&state).await.unwrap();

        // Assert
        // Directory allocation can grow when adding entries; the target must not count.
        assert!(allocated < 1024 * 1024);
    }

    #[tokio::test]
    async fn it_removes_leases_without_seen_bssids_and_preserves_other_state() {
        // Arrange
        let dir = TempDir::new().await.unwrap();
        for name in [
            "old.lease",
            "secret_key",
            "NetworkManager.state",
            "timestamps",
        ] {
            fs::write(dir.join(name), name).await.unwrap();
        }
        let nested = dir.join("nested");
        fs::create_dir(&nested).await.unwrap();
        fs::write(nested.join("keep.lease"), "keep").await.unwrap();
        fs::symlink(dir.join("secret_key"), dir.join("linked.lease"))
            .await
            .unwrap();

        // Act
        remove_disposable_files(dir.as_ref()).await.unwrap();
        remove_disposable_files(dir.as_ref()).await.unwrap();

        // Assert
        assert!(!fs::try_exists(dir.join("old.lease")).await.unwrap());
        for name in ["secret_key", "NetworkManager.state", "timestamps"] {
            assert_eq!(fs::read_to_string(dir.join(name)).await.unwrap(), name);
        }
        assert_eq!(
            fs::read_to_string(nested.join("keep.lease")).await.unwrap(),
            "keep"
        );
        assert!(fs::symlink_metadata(dir.join("linked.lease"))
            .await
            .unwrap()
            .is_symlink());

        // Arrange
        fs::write(dir.join("seen-bssids"), "history").await.unwrap();

        // Act
        remove_disposable_files(dir.as_ref()).await.unwrap();

        // Assert
        assert!(!fs::try_exists(dir.join("seen-bssids")).await.unwrap());
    }

    #[tokio::test]
    async fn it_handles_missing_files_and_not_aborts() {
        // Arrange
        let dir = TempDir::new().await.unwrap();
        let lease = dir.join("old.lease");
        fs::write(&lease, "lease").await.unwrap();

        // Act
        remove_files(vec![dir.join("already-removed.lease"), lease.clone()])
            .await
            .unwrap();
        remove_disposable_files(&dir.join("missing-varlib"))
            .await
            .unwrap();

        // Assert
        assert!(!fs::try_exists(lease).await.unwrap());
    }

    #[tokio::test]
    async fn it_reports_context_on_failure_and_continues_deletion() {
        // Arrange
        let dir = TempDir::new().await.unwrap();
        let invalid = dir.join("not-a-file.lease");
        fs::create_dir(&invalid).await.unwrap();
        let lease = dir.join("old.lease");
        fs::write(&lease, "lease").await.unwrap();

        // Act
        let error = remove_files(vec![invalid.clone(), lease.clone()])
            .await
            .unwrap_err();

        // Assert
        let error = format!("{error:?}");
        assert!(
            error.contains("1 files removed, 1 deletions failed"),
            "{error}"
        );
        assert!(error.contains("failed to remove NM state file"), "{error}");
        assert!(error.contains(invalid.to_str().unwrap()), "{error}");
        assert!(!fs::try_exists(lease).await.unwrap());
        assert!(fs::try_exists(invalid).await.unwrap());
    }

    #[tokio::test]

    async fn it_reports_invalid_state_dir_path() {
        // Arrange
        let dir = TempDir::new().await.unwrap();
        let missing = dir.join("missing");

        // Act
        let error = allocated_size(&missing).await.unwrap_err();

        // Assert
        assert!(format!("{error:?}").contains(missing.to_str().unwrap()));

        // Arrange
        let link = dir.join("linked-varlib");
        fs::symlink(dir.as_ref(), &link).await.unwrap();

        // Act
        let size_result = allocated_size(&link).await;
        let cleanup_result = remove_disposable_files(&link).await;

        // Assert
        assert!(size_result.is_err());
        assert!(cleanup_result.is_err());
    }
}
