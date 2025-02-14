use color_eyre::Result;
use std::{fmt::Display, str::FromStr};

use crate::{from_env, from_file_blocking, OrbInfoError};

#[cfg(test)]
const JABIL_ID_PATH: &str = "./test_jabil_id";
#[cfg(not(test))]
const JABIL_ID_PATH: &str = "/usr/persistent/jabil-id";

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct OrbJabilId(pub String);

impl OrbJabilId {
    #[cfg(feature = "async")]
    pub async fn read() -> Result<Self, OrbInfoError> {
        use crate::from_file;

        let id = if let Ok(s) = from_env("ORB_JABIL_ID") {
            Ok(s.trim().to_string())
        } else {
            let path =
                from_env("ORB_JABIL_ID_PATH").unwrap_or(JABIL_ID_PATH.to_string());
            from_file(&path).await
        }?;
        Ok(Self(id))
    }

    pub fn read_blocking() -> Result<Self, OrbInfoError> {
        let id = if let Ok(s) = from_env("ORB_JABIL_ID") {
            Ok(s.trim().to_string())
        } else {
            let path =
                from_env("ORB_JABIL_ID_PATH").unwrap_or(JABIL_ID_PATH.to_string());
            from_file_blocking(&path)
        }?;
        Ok(Self(id))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

impl FromStr for OrbJabilId {
    type Err = OrbInfoError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(s.to_string()))
    }
}

impl Display for OrbJabilId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::fs;
    use std::path::Path;

    #[tokio::test]
    #[serial]
    async fn test_sync_get_from_env() {
        std::env::set_var("ORB_JABIL_ID", "TEST123");
        let jabil_id = OrbJabilId::read_blocking().unwrap();
        assert_eq!(jabil_id.as_str(), "TEST123");
        std::env::remove_var("ORB_JABIL_ID");
    }

    #[cfg(feature = "async")]
    #[tokio::test]
    #[serial]
    async fn test_async_get_from_env() {
        std::env::set_var("ORB_JABIL_ID", "TEST123");
        let jabil_id = OrbJabilId::read().await.unwrap();
        assert_eq!(jabil_id.as_str(), "TEST123");
        std::env::remove_var("ORB_JABIL_ID");
    }

    #[tokio::test]
    #[serial]
    async fn test_sync_get_from_file() {
        std::env::remove_var("ORB_JABIL_ID");
        std::env::set_var("ORB_JABIL_ID_PATH", "/tmp/jabil-id");

        let test_path = Path::new("/tmp/jabil-id");
        if !test_path.exists() {
            fs::create_dir_all("/tmp").unwrap();
            fs::write(test_path, "FILE456\n").unwrap();
        }

        let jabil_id = OrbJabilId::read_blocking().unwrap();
        assert_eq!(jabil_id.as_str(), "FILE456");

        if test_path.exists() {
            fs::remove_file(test_path).unwrap();
        }
    }

    #[cfg(feature = "async")]
    #[tokio::test]
    #[serial]
    async fn test_async_get_from_file() {
        std::env::remove_var("ORB_JABIL_ID");
        std::env::set_var("ORB_JABIL_ID_PATH", "/tmp/jabil-id");

        let test_path = Path::new("/tmp/jabil-id");
        if !test_path.exists() {
            fs::create_dir_all("/tmp").unwrap();
            fs::write(test_path, "FILE456\n").unwrap();
        }

        let jabil_id = OrbJabilId::read().await.unwrap();
        assert_eq!(jabil_id.as_str(), "FILE456");

        if test_path.exists() {
            fs::remove_file(test_path).unwrap();
        }
    }

    #[tokio::test]
    #[serial]
    async fn test_sync_error_when_not_found() {
        std::env::remove_var("ORB_JABIL_ID");

        let test_path = Path::new("/usr/persistent/jabil-id");
        if test_path.exists() {
            fs::remove_file(test_path).unwrap();
        }

        let jabil_id = OrbJabilId::read_blocking();
        assert!(matches!(jabil_id, Err(OrbInfoError::IoErr(_))));
    }

    #[cfg(feature = "async")]
    #[tokio::test]
    #[serial]
    async fn test_async_error_when_not_found() {
        std::env::remove_var("ORB_JABIL_ID");

        let test_path = Path::new("/usr/persistent/jabil-id");
        if test_path.exists() {
            fs::remove_file(test_path).unwrap();
        }

        let jabil_id = OrbJabilId::read().await;
        assert!(matches!(jabil_id, Err(OrbInfoError::IoErr(_))));
    }
}
