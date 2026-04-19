use color_eyre::Result;
use eyre::{Context, ContextCompat};

pub async fn orb_uptime() -> Result<f64> {
    orb_uptime_from_path("/proc/uptime").await
}

async fn orb_uptime_from_path(path: &str) -> Result<f64> {
    let uptime = tokio::fs::read_to_string(path)
        .await
        .wrap_err("failed to read uptime from procs")?;

    let uptime = uptime
        .split_whitespace()
        .next()
        .wrap_err_with(|| format!("failed to split whitespace in uptime: {uptime}"))?;

    uptime
        .parse::<f64>()
        .wrap_err_with(|| format!("failed to parse uptime: {uptime}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_orb_uptime_from_path() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("uptime");
        tokio::fs::write(&file_path, "123.45 678.90").await.unwrap();
        assert_eq!(
            orb_uptime_from_path(file_path.to_str().unwrap())
                .await
                .unwrap(),
            123.45
        );
    }

    #[tokio::test]
    async fn test_orb_uptime_from_path_invalid_content() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("uptime");
        tokio::fs::write(&file_path, "invalid").await.unwrap();
        assert!(orb_uptime_from_path(file_path.to_str().unwrap())
            .await
            .is_err());
    }

    #[tokio::test]
    async fn test_orb_uptime_from_path_empty() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("uptime");
        tokio::fs::write(&file_path, "").await.unwrap();
        assert!(orb_uptime_from_path(file_path.to_str().unwrap())
            .await
            .is_err());
    }

    #[tokio::test]
    async fn test_orb_uptime_from_path_file_not_found() {
        assert!(orb_uptime_from_path("nonexistent").await.is_err());
    }
}
