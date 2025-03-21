pub mod diff_ota;
pub mod fetch;
pub mod file_or_stdout;
pub mod ota_path;

use std::path::Path;

use color_eyre::Result;

pub async fn is_empty_dir(d: &Path) -> Result<bool> {
    Ok(tokio::fs::read_dir(d).await?.next_entry().await?.is_none())
}

#[cfg(test)]
mod test {
    use super::*;

    #[tokio::test]
    async fn test_empty_dir() {
        let empty = tempfile::tempdir().unwrap();
        assert!(is_empty_dir(empty.path())
            .await
            .expect("dir exists so reading should work"))
    }

    #[tokio::test]
    async fn test_populated_dir() {
        let populated = tempfile::tempdir().unwrap();
        tokio::fs::create_dir(populated.path().join("foo"))
            .await
            .unwrap();
        assert!(!is_empty_dir(populated.path())
            .await
            .expect("dir exists so reading should work"))
    }

    #[tokio::test]
    async fn test_missing_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("missing");
        assert!(
            is_empty_dir(&missing).await.is_err(),
            "expected an error because dir doesn't exist"
        );
    }
}
