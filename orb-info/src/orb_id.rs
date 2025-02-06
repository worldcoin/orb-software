use color_eyre::Result;
use std::sync::RwLock;

use crate::OrbInfoError;

#[derive(Debug)]
pub struct OrbId {
    id: RwLock<Option<String>>,
}

impl OrbId {
    #[must_use]
    pub fn new() -> Self {
        Self {
            id: RwLock::new(None),
        }
    }

    pub async fn get(&self) -> Result<String, OrbInfoError> {
        if let Some(orb_id) = self.id.read().unwrap().clone() {
            return Ok(orb_id);
        }
        let id = if let Some(s) = std::env::var("ORB_ID").ok() {
            Ok(s.trim().to_string())
        } else {
            let output = tokio::process::Command::new("orb-id")
                .output()
                .await
                .map_err(|e| OrbInfoError::IoErr(e))?;
            assert!(output.status.success(), "orb-id binary failed");
            match String::from_utf8(output.stdout) {
                Ok(s) => Ok(s.trim().to_string()),
                Err(e) => Err(OrbInfoError::Utf8Err(e)),
            }
        }?;
        *self.id.write().unwrap() = Some(id.clone());
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[tokio::test]
    #[serial_test::serial]
    async fn test_get_orb_id_from_env() {
        let test_id = "test-orb-123";
        env::set_var("ORB_ID", test_id);

        let orb_id = OrbId::new();

        let result = orb_id.get().await.unwrap();
        assert_eq!(result, test_id);

        // Test caching works
        let cached_result = orb_id.get().await.unwrap();
        assert_eq!(cached_result, test_id);

        env::remove_var("ORB_ID");
    }

    #[tokio::test]
    #[serial_test::serial]
    #[should_panic(expected = "IoErr")]
    async fn test_get_orb_id_binary_failure() {
        env::remove_var("ORB_ID");

        let orb_id = OrbId::new();

        // This should panic since orb-id binary likely doesn't exist in test environment
        let _ = orb_id.get().await.unwrap();
    }
}
