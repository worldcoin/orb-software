use color_eyre::Result;
use std::sync::RwLock;

use crate::OrbInfoError;

#[derive(Debug)]
pub struct OrbJabilId {
    jabil_id: RwLock<Option<String>>,
}

impl OrbJabilId {
    #[must_use]
    pub fn new() -> Self {
        Self {
            jabil_id: RwLock::new(None),
        }
    }

    pub async fn get(&self) -> Result<String, OrbInfoError> {
        if let Some(jabil_id) = self.jabil_id.read().unwrap().clone() {
            return Ok(jabil_id);
        }
        let jabil_id = if let Some(s) = std::env::var("ORB_JABIL_ID").ok() {
            Ok(s.trim().to_string())
        } else {
            let path = if let Some(s) = std::env::var("ORB_JABIL_ID_PATH").ok() {
                s.trim().to_string()
            } else {
                "/usr/persistent/jabil-id".to_string()
            };
            match std::fs::read_to_string(path) {
                Ok(s) => Ok(s.trim().to_string()),
                Err(e) => Err(OrbInfoError::IoErr(e)),
            }
        }?;
        *self.jabil_id.write().unwrap() = Some(jabil_id.clone());
        Ok(jabil_id)
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
        std::env::set_var("ORB_JABIL_ID", "TEST123");
        let jabil_id = OrbJabilId::new();
        assert_eq!(jabil_id.get().await.unwrap(), "TEST123");
        std::env::remove_var("ORB_JABIL_ID");
    }

    #[tokio::test]
    #[serial]
    async fn test_get_from_file() {
        std::env::remove_var("ORB_JABIL_ID");
        std::env::set_var("ORB_JABIL_ID_PATH", "/tmp/jabil-id");

        let test_path = Path::new("/tmp/jabil-id");
        if !test_path.exists() {
            fs::create_dir_all("/tmp").unwrap();
            fs::write(test_path, "FILE456\n").unwrap();
        }

        let jabil_id = OrbJabilId::new();
        assert_eq!(jabil_id.get().await.unwrap(), "FILE456");

        if test_path.exists() {
            fs::remove_file(test_path).unwrap();
        }
    }

    #[tokio::test]
    #[serial]
    async fn test_caching() {
        std::env::set_var("ORB_JABIL_ID", "CACHE789");
        let jabil_id = OrbJabilId::new();

        // First call should read from env
        assert_eq!(jabil_id.get().await.unwrap(), "CACHE789");

        // Remove env var
        std::env::remove_var("ORB_JABIL_ID");

        // Second call should return cached value
        assert_eq!(jabil_id.get().await.unwrap(), "CACHE789");
    }

    #[tokio::test]
    #[serial]
    async fn test_error_when_not_found() {
        std::env::remove_var("ORB_JABIL_ID");

        let test_path = Path::new("/usr/persistent/jabil-id");
        if test_path.exists() {
            fs::remove_file(test_path).unwrap();
        }

        let jabil_id = OrbJabilId::new();
        assert!(matches!(jabil_id.get().await, Err(OrbInfoError::IoErr(_))));
    }
}
