use color_eyre::Result;
use eyre::{Context, ContextCompat};
use std::path::Path;

pub async fn orb_boot_id(procfs: &Path) -> Result<String> {
    orb_boot_id_from_path(
        &procfs
            .join("sys")
            .join("kernel")
            .join("random")
            .join("boot_id"),
    )
    .await
}

async fn orb_boot_id_from_path(path: &Path) -> Result<String> {
    let boot_id = tokio::fs::read_to_string(path)
        .await
        .wrap_err("failed to read boot-id from procfs")?;

    let boot_id = boot_id
        .split_whitespace()
        .next()
        .wrap_err_with(|| format!("failed to parse boot-id: {boot_id}"))?;

    if boot_id.is_empty() {
        eyre::bail!("boot-id was empty");
    }

    Ok(boot_id.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_orb_boot_id_from_path() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("boot_id");
        tokio::fs::write(&file_path, "0f0e0d0c-0b0a-0908-0706-050403020100\n")
            .await
            .unwrap();

        assert_eq!(
            orb_boot_id_from_path(&file_path).await.unwrap(),
            "0f0e0d0c-0b0a-0908-0706-050403020100"
        );
    }

    #[tokio::test]
    async fn test_orb_boot_id_from_path_empty() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("boot_id");
        tokio::fs::write(&file_path, "").await.unwrap();

        assert!(orb_boot_id_from_path(&file_path).await.is_err());
    }

    #[tokio::test]
    async fn test_orb_boot_id_from_path_missing() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("missing_boot_id");

        assert!(orb_boot_id_from_path(&file_path).await.is_err());
    }
}
