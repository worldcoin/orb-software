use std::{fmt::Display, str::FromStr};
use thiserror::Error;

use crate::from_file_blocking;

#[cfg(test)]
const JABIL_ID_PATH: &str = "./test_jabil_id";
#[cfg(not(test))]
const JABIL_ID_PATH: &str = "/usr/persistent/jabil-id";

#[derive(Debug, Error)]
pub enum ReadErr {
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct OrbJabilId(pub String);

impl OrbJabilId {
    #[cfg(feature = "async")]
    pub async fn read() -> Result<Self, ReadErr> {
        use crate::from_file;

        let id = if let Ok(s) = std::env::var("ORB_JABIL_ID") {
            Ok(s.trim().to_string())
        } else {
            let path =
                std::env::var("ORB_JABIL_ID_PATH").unwrap_or(JABIL_ID_PATH.to_string());
            from_file(&path).await
        }?;
        Ok(Self(id))
    }

    pub fn read_blocking() -> Result<Self, ReadErr> {
        let id = if let Ok(s) = std::env::var("ORB_JABIL_ID") {
            Ok(s.trim().to_string())
        } else {
            let path =
                std::env::var("ORB_JABIL_ID_PATH").unwrap_or(JABIL_ID_PATH.to_string());
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
    type Err = hex::FromHexError;

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
    use std::path::Path;

    #[test]
    #[serial]
    fn test_sync_get_from_env() {
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

    #[test]
    #[serial]
    fn test_sync_get_from_file() {
        std::env::remove_var("ORB_JABIL_ID");
        std::env::set_var("ORB_JABIL_ID_PATH", "/tmp/jabil-id");

        let test_path = Path::new("/tmp/jabil-id");
        if !test_path.exists() {
            std::fs::create_dir_all("/tmp").unwrap();
            std::fs::write(test_path, "FILE456\n").unwrap();
        }

        let jabil_id = OrbJabilId::read_blocking().unwrap();
        assert_eq!(jabil_id.as_str(), "FILE456");

        if test_path.exists() {
            std::fs::remove_file(test_path).unwrap();
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
            tokio::fs::create_dir_all("/tmp").await.unwrap();
            tokio::fs::write(test_path, "FILE456\n").await.unwrap();
        }

        let jabil_id = OrbJabilId::read().await.unwrap();
        assert_eq!(jabil_id.as_str(), "FILE456");

        if test_path.exists() {
            tokio::fs::remove_file(test_path).await.unwrap();
        }
    }

    #[test]
    #[serial]
    fn test_sync_error_when_not_found() {
        std::env::remove_var("ORB_JABIL_ID");

        let test_path = Path::new("/usr/persistent/jabil-id");
        if test_path.exists() {
            std::fs::remove_file(test_path).unwrap();
        }

        let jabil_id = OrbJabilId::read_blocking();
        let Err(ReadErr::Io(io_err)) = jabil_id else {
            panic!("expected an IO error");
        };
        assert_eq!(io_err.kind(), std::io::ErrorKind::NotFound);
    }

    #[cfg(feature = "async")]
    #[tokio::test]
    #[serial]
    async fn test_async_error_when_not_found() {
        std::env::remove_var("ORB_JABIL_ID");

        let test_path = Path::new("/usr/persistent/jabil-id");
        if test_path.exists() {
            tokio::fs::remove_file(test_path).await.unwrap();
        }

        let jabil_id = OrbJabilId::read().await;
        let Err(ReadErr::Io(io_err)) = jabil_id else {
            panic!("expected an IO error");
        };
        assert_eq!(io_err.kind(), std::io::ErrorKind::NotFound);
    }
}
