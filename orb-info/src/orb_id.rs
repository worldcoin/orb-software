use color_eyre::Result;
use std::sync::Arc;

use crate::{from_binary_blocking, from_env, OrbInfoError};

#[derive(Debug, Default, Clone, Eq, PartialEq, Hash)]
pub struct OrbId {
    id: Arc<String>,
}

impl OrbId {
    #[cfg(feature = "async")]
    pub async fn read() -> Result<Self, OrbInfoError> {
        use crate::from_binary;

        let id = if let Ok(s) = from_env("ORB_ID") {
            Ok(s.trim().to_string())
        } else {
            from_binary("orb-id").await
        }?;
        Ok(Self { id: Arc::new(id) })
    }

    pub fn read_blocking() -> Result<Self, OrbInfoError> {
        let id = if let Ok(s) = from_env("ORB_ID") {
            Ok(s.trim().to_string())
        } else {
            from_binary_blocking("orb-id")
        }?;
        Ok(Self { id: Arc::new(id) })
    }

    pub fn value(&self) -> &str {
        self.id.as_str()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[tokio::test]
    #[serial_test::serial]
    async fn test_sync_get_orb_id_from_env() {
        let test_id = "test-orb-123";
        env::set_var("ORB_ID", test_id);

        let orb_id = OrbId::read_blocking().unwrap();
        assert_eq!(orb_id.value(), test_id);

        // Test caching works
        let cached_result = orb_id.value();
        assert_eq!(cached_result, test_id);

        env::remove_var("ORB_ID");
    }

    #[cfg(feature = "async")]
    #[tokio::test]
    #[serial_test::serial]
    async fn test_async_get_orb_id_from_env() {
        let test_id = "test-orb-123";
        env::set_var("ORB_ID", test_id);

        let orb_id = OrbId::read().await.unwrap();
        assert_eq!(orb_id.value(), test_id);

        // Test caching works
        let cached_result = orb_id.value();
        assert_eq!(cached_result, test_id);

        env::remove_var("ORB_ID");
    }

    #[tokio::test]
    #[serial_test::serial]
    #[should_panic(expected = "IoErr")]
    async fn test_sync_get_orb_id_binary_failure() {
        env::remove_var("ORB_ID");

        // This should panic since orb-id binary likely doesn't exist in test environment
        let _orb_id = OrbId::read_blocking().unwrap();
    }

    #[cfg(feature = "async")]
    #[tokio::test]
    #[serial_test::serial]
    #[should_panic(expected = "IoErr")]
    async fn test_async_get_orb_id_binary_failure() {
        env::remove_var("ORB_ID");

        // This should panic since orb-id binary likely doesn't exist in test environment
        let _orb_id = OrbId::read().await.unwrap();
    }
}
