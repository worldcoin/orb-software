pub mod diff_ota;
mod diff_plan;
pub mod fetch;
pub mod file_or_stdout;
pub mod ota_path;

use std::path::Path;

use color_eyre::Result;

pub async fn is_empty_dir(d: &Path) -> Result<bool> {
    Ok(tokio::fs::read_dir(d).await?.next_entry().await?.is_none())
}

trait PathExt {
    async fn is_dir_async(&self) -> bool;
    async fn is_file_async(&self) -> bool;
    async fn exists_async(&self) -> bool;
}

impl<P: AsRef<Path>> PathExt for P {
    async fn is_dir_async(&self) -> bool {
        let Ok(mdata) = tokio::fs::metadata(self).await else {
            return false;
        };

        mdata.is_dir()
    }

    async fn is_file_async(&self) -> bool {
        let Ok(mdata) = tokio::fs::metadata(self).await else {
            return false;
        };

        mdata.is_file()
    }

    async fn exists_async(&self) -> bool {
        tokio::fs::try_exists(self).await.unwrap_or(false)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use async_tempfile::TempDir;

    #[tokio::test]
    async fn test_empty_dir() {
        let empty = TempDir::new().await.unwrap();
        assert!(is_empty_dir(empty.dir_path())
            .await
            .expect("dir exists so reading should work"))
    }

    #[tokio::test]
    async fn test_populated_dir() {
        let populated = TempDir::new().await.unwrap();
        tokio::fs::create_dir(populated.dir_path().join("foo"))
            .await
            .unwrap();
        assert!(!is_empty_dir(populated.dir_path())
            .await
            .expect("dir exists so reading should work"))
    }

    #[tokio::test]
    async fn test_missing_dir() {
        let tmp = TempDir::new().await.unwrap();
        let missing = tmp.dir_path().join("missing");
        assert!(
            is_empty_dir(&missing).await.is_err(),
            "expected an error because dir doesn't exist"
        );
    }
}
