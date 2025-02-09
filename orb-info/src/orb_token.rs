use color_eyre::eyre::Result;
use derive_more::{Display, From};
use tokio::sync::watch;
use zbus::export::futures_util::StreamExt;
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

/// Authorization token to the backend. Comes from short lived token daemon.
#[derive(Debug, Display, From, Clone)]
pub struct Token(String);

#[derive(Debug)]
pub struct OrbToken {
    auth_token_receiver: watch::Receiver<Token>,
    join_handle: tokio::task::JoinHandle<()>,
}

impl OrbToken {
    pub async fn new() -> Result<Self, OrbInfoError> {
        let (join_handle, auth_token_receiver) = Self::setup_dbus().await?;
        Ok(OrbToken {
            auth_token_receiver,
            join_handle,
        })
    }

    pub async fn get_orb_token(&self) -> Result<String, OrbInfoError> {
        let reply = self.auth_token_receiver.borrow().to_owned();
        Ok(reply.to_string())
    }

    async fn setup_dbus(
    ) -> Result<(tokio::task::JoinHandle<()>, watch::Receiver<Token>), OrbInfoError>
    {
        let connection = Connection::session().await.map_err(OrbInfoError::ZbusErr)?;

        let auth_token_proxy = AuthTokenProxy::new(&connection)
            .await
            .map_err(OrbInfoError::ZbusErr)?;

        let initial_value = Token::from(auth_token_proxy.token().await.unwrap());
        let (send, recv) = watch::channel(initial_value);
        let auth_token_proxy = auth_token_proxy.clone();
        let join_handle = tokio::task::spawn(async move {
            let mut token_changed = auth_token_proxy.receive_token_changed().await;
            while let Some(token) = token_changed.next().await {
                let token = token.get().await.expect("should have received token");
                send.send(Token::from(token))
                    .expect("should have sent token to watchers");
            }
        });
        Ok((join_handle, recv))
    }
}

impl Drop for OrbToken {
    fn drop(&mut self) {
        // Abort the background task when this object goes out of scope
        self.join_handle.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::Arc, time::Duration};
    use tokio::sync::Mutex;
    use zbus::{object_server::SignalContext, ConnectionBuilder};

    #[derive(Clone)]
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

    async fn setup_test_server() -> Result<(Connection, TestAuthTokenManager)> {
        let token = Arc::new(Mutex::new("test_token".to_string()));
        let test_manager = TestAuthTokenManager { token };

        let connection = ConnectionBuilder::session()?
            .name("org.worldcoin.AuthTokenManager1")?
            .serve_at("/org/worldcoin/AuthTokenManager1", test_manager.clone())?
            .build()
            .await?;

        Ok((connection, test_manager))
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_get_orb_token() -> Result<()> {
        let (connection, test_manager) = setup_test_server().await?;

        // Create client
        let orb_token = OrbToken::new().await.expect("should have token");

        // Test getting token
        let retrieved_token = orb_token.get_orb_token().await?;
        assert_eq!(retrieved_token, test_manager.token().await);

        connection.close().await?;
        Ok(())
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_token_update() -> Result<()> {
        let (connection, test_manager) = setup_test_server().await?;

        // Create client
        let orb_token = OrbToken::new().await.expect("should have token");

        // Get initial token
        let initial_token = orb_token.get_orb_token().await?;
        assert_eq!(initial_token, "test_token");

        // Update token via proxy
        {
            let mut token_guard = test_manager.token.lock().await;
            *token_guard = "updated_token".to_string();
        }
        test_manager
            .token_changed(
                &SignalContext::new(&connection, "/org/worldcoin/AuthTokenManager1")
                    .unwrap(),
            )
            .await?;

        // Verify updated token is retrieved
        tokio::time::sleep(Duration::from_millis(100)).await;
        let updated_token = orb_token.get_orb_token().await?;
        assert_eq!(updated_token, "updated_token");

        connection.close().await?;
        Ok(())
    }
}
