use color_eyre::Result;
use std::sync::RwLock;

use crate::{from_env, from_file, OrbInfoError};

#[derive(Debug, Default)]
pub struct OrbName {
    name: RwLock<Option<String>>,
}

impl OrbName {
    #[must_use]
    pub fn new() -> Self {
        Self {
            name: RwLock::new(None),
        }
    }

    pub async fn get(&self) -> Result<String, OrbInfoError> {
        if let Some(orb_name) = self.name.read().unwrap().clone() {
            return Ok(orb_name);
        }
        let orb_name = if let Ok(s) = from_env("ORB_NAME").await {
            Ok(s.trim().to_string())
        } else {
            let path = if let Ok(s) = from_env("ORB_NAME_PATH").await {
                s.trim().to_string()
            } else {
                "/usr/persistent/orb-name".to_string()
            };
            from_file(&path).await
        }?;
        *self.name.write().unwrap() = Some(orb_name.clone());
        Ok(orb_name)
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
    async fn test_get_from_env() {
        std::env::set_var("ORB_NAME", "TEST_ORB");
        let orb_name = OrbName {
            name: RwLock::new(None),
        };
        assert_eq!(orb_name.get().await.unwrap(), "TEST_ORB");
        std::env::remove_var("ORB_NAME");
    }

    #[tokio::test]
    #[serial]
    async fn test_get_from_file() {
        std::env::remove_var("ORB_NAME");
        std::env::set_var("ORB_NAME_PATH", "/tmp/orb-name");

        let test_path = Path::new("/tmp/orb-name");
        if !test_path.exists() {
            fs::create_dir_all("/tmp").unwrap();
            fs::write(test_path, "FILE_ORB\n").unwrap();
        }

        let orb_name = OrbName {
            name: RwLock::new(None),
        };
        assert_eq!(orb_name.get().await.unwrap(), "FILE_ORB");

        if test_path.exists() {
            fs::remove_file(test_path).unwrap();
        }
    }

    #[tokio::test]
    #[serial]
    async fn test_caching() {
        std::env::set_var("ORB_NAME", "CACHE_ORB");
        let orb_name = OrbName {
            name: RwLock::new(None),
        };

        // First call should read from env
        assert_eq!(orb_name.get().await.unwrap(), "CACHE_ORB");

        // Remove env var
        std::env::remove_var("ORB_NAME");

        // Second call should return cached value
        assert_eq!(orb_name.get().await.unwrap(), "CACHE_ORB");
    }

    #[tokio::test]
    #[serial]
    async fn test_error_when_not_found() {
        std::env::remove_var("ORB_NAME");

        let test_path = Path::new("/usr/persistent/orb-name");
        if test_path.exists() {
            fs::remove_file(test_path).unwrap();
        }

        let orb_name = OrbName {
            name: RwLock::new(None),
        };
        assert!(matches!(orb_name.get().await, Err(OrbInfoError::IoErr(_))));
    }
}
