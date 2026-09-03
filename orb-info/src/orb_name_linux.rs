use std::{fmt::Display, str::FromStr};
use thiserror::Error;

use crate::from_file_blocking;

#[cfg(test)]
const ORB_NAME_PATH: &str = "./test_orb_name";
#[cfg(not(test))]
const ORB_NAME_PATH: &str = "/usr/persistent/orb-name";

#[derive(Debug, Error)]
pub enum ReadErr {
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct OrbName(pub String);

impl OrbName {
    #[cfg(feature = "async")]
    pub async fn read() -> Result<Self, ReadErr> {
        use crate::from_file;

        let name = if let Ok(s) = std::env::var("ORB_NAME") {
            Ok(s.trim().to_string())
        } else {
            let path =
                std::env::var("ORB_NAME_PATH").unwrap_or(ORB_NAME_PATH.to_owned());
            from_file(&path).await
        }?;
        Ok(Self(name))
    }

    pub fn read_blocking() -> Result<Self, ReadErr> {
        let name = if let Ok(s) = std::env::var("ORB_NAME") {
            Ok(s.trim().to_string())
        } else {
            let path =
                std::env::var("ORB_NAME_PATH").unwrap_or(ORB_NAME_PATH.to_string());
            from_file_blocking(&path)
        }?;
        Ok(Self(name))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

impl FromStr for OrbName {
    type Err = hex::FromHexError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(s.to_string()))
    }
}

impl Display for OrbName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::path::Path;

    #[cfg(feature = "async")]
    #[tokio::test]
    #[serial]
    async fn test_async_get_from_env() {
        std::env::set_var("ORB_NAME", "TEST_ORB");
        let orb_name = OrbName::read().await.unwrap();
        assert_eq!(orb_name.as_str(), "TEST_ORB");
        std::env::remove_var("ORB_NAME");
    }

    #[test]
    #[serial]
    fn test_sync_get_from_env() {
        std::env::set_var("ORB_NAME", "TEST_ORB");
        let orb_name = OrbName::read_blocking().unwrap();
        assert_eq!(orb_name.as_str(), "TEST_ORB");
        std::env::remove_var("ORB_NAME");
    }

    #[cfg(feature = "async")]
    #[tokio::test]
    #[serial]
    async fn test_async_get_from_file() {
        std::env::remove_var("ORB_NAME");
        std::env::set_var("ORB_NAME_PATH", "/tmp/orb-name");

        let test_path = Path::new("/tmp/orb-name");
        if !test_path.exists() {
            tokio::fs::create_dir_all("/tmp").await.unwrap();
            tokio::fs::write(test_path, "FILE_ORB\n").await.unwrap();
        }

        let orb_name = OrbName::read().await.unwrap();
        assert_eq!(orb_name.as_str(), "FILE_ORB");

        if test_path.exists() {
            tokio::fs::remove_file(test_path).await.unwrap();
        }
    }

    #[test]
    #[serial]
    fn test_sync_get_from_file() {
        std::env::remove_var("ORB_NAME");
        std::env::set_var("ORB_NAME_PATH", "/tmp/orb-name");

        let test_path = Path::new("/tmp/orb-name");
        if !test_path.exists() {
            std::fs::create_dir_all("/tmp").unwrap();
            std::fs::write(test_path, "FILE_ORB\n").unwrap();
        }

        let orb_name = OrbName::read_blocking().unwrap();
        assert_eq!(orb_name.as_str(), "FILE_ORB");

        if test_path.exists() {
            std::fs::remove_file(test_path).unwrap();
        }
    }

    #[test]
    #[serial]
    fn test_sync_error_when_not_found() {
        std::env::remove_var("ORB_NAME");

        // TODO(paulquinn00): Use a temporary path, in case we run this on an orb.
        let test_path = Path::new("/usr/persistent/orb-name");
        if test_path.exists() {
            std::fs::remove_file(test_path).unwrap();
        }

        let orb_name = OrbName::read_blocking();
        let Err(ReadErr::Io(io_err)) = orb_name else {
            panic!("expected an IO error");
        };
        assert_eq!(io_err.kind(), std::io::ErrorKind::NotFound);
    }

    #[cfg(feature = "async")]
    #[tokio::test]
    #[serial]
    async fn test_async_error_when_not_found() {
        std::env::remove_var("ORB_NAME");

        // TODO(paulquinn00): Use a temporary path, in case we run this on an orb.
        let test_path = Path::new("/usr/persistent/orb-name");
        if test_path.exists() {
            tokio::fs::remove_file(test_path).await.unwrap();
        }

        let orb_name = OrbName::read().await;
        let Err(ReadErr::Io(io_err)) = orb_name else {
            panic!("expected an IO error");
        };
        assert_eq!(io_err.kind(), std::io::ErrorKind::NotFound);
    }
}
