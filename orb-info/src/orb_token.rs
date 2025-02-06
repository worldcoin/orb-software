use color_eyre::eyre::Result;
use zbus::{proxy, Connection};

use crate::OrbInfoError;

#[proxy(
    default_service = "org.worldcoin.AuthTokenManager1",
    default_path = "/org/worldcoin/AuthTokenManager1",
    interface = "org.worldcoin.AuthTokenManager1"
)]
trait AuthToken {
    #[zbus(property)]
    fn token(&self) -> zbus::Result<String>;
}

#[derive(Debug)]
pub struct OrbToken {
    auth_token_proxy: AuthTokenProxy<'static>,
}

impl OrbToken {
    #[must_use]
    pub async fn new() -> Result<Self, OrbInfoError> {
        let auth_token_proxy = Self::setup_dbus().await?;
        Ok(OrbToken { auth_token_proxy })
    }

    pub async fn get_orb_token(&self) -> Result<String, OrbInfoError> {
        let reply = self
            .auth_token_proxy
            .token()
            .await
            .map_err(|e| OrbInfoError::ZbusErr(e))?;
        Ok(reply)
    }

    async fn setup_dbus() -> Result<AuthTokenProxy<'static>, OrbInfoError> {
        let connection = Connection::session()
            .await
            .map_err(|e| OrbInfoError::ZbusErr(e))?;

        let auth_token_proxy = AuthTokenProxy::new(&connection)
            .await
            .map_err(|e| OrbInfoError::ZbusErr(e))?;
        Ok(auth_token_proxy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::Mutex;
    use zbus::ConnectionBuilder;

    struct TestAuthTokenManager {
        token: Arc<Mutex<String>>,
    }

    #[zbus::interface(name = "org.worldcoin.AuthTokenManager1")]
    impl TestAuthTokenManager {
        #[zbus(property)]
        async fn token(&self) -> String {
            self.token.lock().await.clone()
        }
    }

    async fn setup_test_server() -> Result<(Connection, Arc<Mutex<String>>)> {
        let token = Arc::new(Mutex::new("test_token".to_string()));
        let token_clone = token.clone();

        let test_manager = TestAuthTokenManager { token };

        let connection = ConnectionBuilder::session()?
            .name("org.worldcoin.AuthTokenManager1")?
            .serve_at("/org/worldcoin/AuthTokenManager1", test_manager)?
            .build()
            .await?;

        Ok((connection, token_clone))
    }

    #[tokio::test]
    async fn test_orb_token() -> Result<()> {
        let (_connection, token) = setup_test_server().await?;

        // Create client
        let orb_token = OrbToken::new().await?;

        // Test getting token
        let retrieved_token = orb_token.get_orb_token().await?;
        assert_eq!(retrieved_token, token.lock().await.clone());

        Ok(())
    }
}
