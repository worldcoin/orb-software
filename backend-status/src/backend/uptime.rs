pub async fn orb_uptime() -> Option<f64> {
    orb_uptime_from_path("/proc/uptime").await
}

async fn orb_uptime_from_path(path: &str) -> Option<f64> {
    let uptime = tokio::fs::read_to_string(path).await.ok()?;
    let uptime = uptime.split_whitespace().next()?;
    uptime.parse::<f64>().ok()
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
            orb_uptime_from_path(file_path.to_str().unwrap()).await,
            Some(123.45)
        );
    }

    #[tokio::test]
    async fn test_orb_uptime_from_path_invalid_content() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("uptime");
        tokio::fs::write(&file_path, "invalid").await.unwrap();
        assert_eq!(
            orb_uptime_from_path(file_path.to_str().unwrap()).await,
            None
        );
    }

    #[tokio::test]
    async fn test_orb_uptime_from_path_empty() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("uptime");
        tokio::fs::write(&file_path, "").await.unwrap();
        assert_eq!(
            orb_uptime_from_path(file_path.to_str().unwrap()).await,
            None
        );
    }

    #[tokio::test]
    async fn test_orb_uptime_from_path_file_not_found() {
        assert_eq!(orb_uptime_from_path("nonexistent").await, None);
    }
}

