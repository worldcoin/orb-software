use color_eyre::Result;
use std::{fmt::Display, str::FromStr, sync::Arc};

use crate::{from_env, from_file_blocking, OrbInfoError};

#[cfg(test)]
const ORB_NAME_PATH: &str = "./test_orb_name";
#[cfg(not(test))]
const ORB_NAME_PATH: &str = "/usr/persistent/orb-name";

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct OrbName {
    name: Arc<String>,
}

impl OrbName {
    #[cfg(feature = "async")]
    pub async fn read() -> Result<Self, OrbInfoError> {
        use crate::from_file;

        let name = if let Ok(s) = from_env("ORB_NAME") {
            Ok(s.trim().to_string())
        } else {
            let path = from_env("ORB_NAME_PATH").unwrap_or(ORB_NAME_PATH.to_string());
            from_file(&path).await
        }?;
        Ok(Self {
            name: Arc::new(name),
        })
    }

    pub fn read_blocking() -> Result<Self, OrbInfoError> {
        let name = if let Ok(s) = from_env("ORB_NAME") {
            Ok(s.trim().to_string())
        } else {
            let path = from_env("ORB_NAME_PATH").unwrap_or(ORB_NAME_PATH.to_string());
            from_file_blocking(&path)
        }?;
        Ok(Self {
            name: Arc::new(name),
        })
    }

    pub fn as_str(&self) -> &str {
        self.name.as_str()
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.name.as_bytes()
    }
}

impl FromStr for OrbName {
    type Err = OrbInfoError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self {
            name: Arc::new(s.to_string()),
        })
    }
}

impl Display for OrbName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.name.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::fs;
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

    #[tokio::test]
    #[serial]
    async fn test_sync_get_from_env() {
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
            fs::create_dir_all("/tmp").unwrap();
            fs::write(test_path, "FILE_ORB\n").unwrap();
        }

        let orb_name = OrbName::read().await.unwrap();
        assert_eq!(orb_name.as_str(), "FILE_ORB");

        if test_path.exists() {
            fs::remove_file(test_path).unwrap();
        }
    }

    #[tokio::test]
    #[serial]
    async fn test_sync_get_from_file() {
        std::env::remove_var("ORB_NAME");
        std::env::set_var("ORB_NAME_PATH", "/tmp/orb-name");

        let test_path = Path::new("/tmp/orb-name");
        if !test_path.exists() {
            fs::create_dir_all("/tmp").unwrap();
            fs::write(test_path, "FILE_ORB\n").unwrap();
        }

        let orb_name = OrbName::read_blocking().unwrap();
        assert_eq!(orb_name.as_str(), "FILE_ORB");

        if test_path.exists() {
            fs::remove_file(test_path).unwrap();
        }
    }

    #[tokio::test]
    #[serial]
    async fn test_sync_error_when_not_found() {
        std::env::remove_var("ORB_NAME");

        let test_path = Path::new("/usr/persistent/orb-name");
        if test_path.exists() {
            fs::remove_file(test_path).unwrap();
        }

        let orb_name = OrbName::read_blocking();
        assert!(matches!(orb_name, Err(OrbInfoError::IoErr(_))));
    }

    #[cfg(feature = "async")]
    #[tokio::test]
    #[serial]
    async fn test_async_error_when_not_found() {
        std::env::remove_var("ORB_NAME");

        let test_path = Path::new("/usr/persistent/orb-name");
        if test_path.exists() {
            fs::remove_file(test_path).unwrap();
        }

        let orb_name = OrbName::read().await;
        assert!(matches!(orb_name, Err(OrbInfoError::IoErr(_))));
    }
}
