use color_eyre::eyre::Result;
use orb_attest_dbus::AuthTokenManagerProxy;
use std::sync::Arc;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use zbus::export::futures_util::StreamExt;
use zbus::Connection;

use crate::OrbInfoError;

#[derive(Debug, Clone)]
pub struct OrbToken {
    pub token_receiver: watch::Receiver<String>,
    pub join_handle: Arc<tokio::task::JoinHandle<()>>,
    pub cancel_token: CancellationToken,
}

impl OrbToken {
    pub async fn read(cancel_token: &CancellationToken) -> Result<Self, OrbInfoError> {
        let cancel_token = cancel_token.child_token();
        let (join_handle, token_receiver) =
            Self::setup_dbus(cancel_token.clone()).await?;
        Ok(OrbToken {
            token_receiver,
            join_handle: Arc::new(join_handle),
            cancel_token,
        })
    }

    pub async fn value(&self) -> Result<String, OrbInfoError> {
        let reply = self.token_receiver.borrow().to_owned();
        Ok(reply.to_string())
    }

    async fn setup_dbus(
        cancel_token: CancellationToken,
    ) -> Result<(tokio::task::JoinHandle<()>, watch::Receiver<String>), OrbInfoError>
    {
        let connection = Connection::session().await.map_err(OrbInfoError::ZbusErr)?;

        let auth_token_proxy = AuthTokenManagerProxy::new(&connection)
            .await
            .map_err(OrbInfoError::ZbusErr)?;

        let initial_value = auth_token_proxy.token().await.unwrap();
        let (send, recv) = watch::channel(initial_value);
        let auth_token_proxy = auth_token_proxy.clone();
        let join_handle =
            tokio::task::spawn(Self::task_inner(auth_token_proxy, send, cancel_token));
        Ok((join_handle, recv))
    }

    async fn task_inner(
        auth_token_proxy: AuthTokenManagerProxy<'_>,
        send: watch::Sender<String>,
        cancel_token: tokio_util::sync::CancellationToken,
    ) {
        let token_updater = async move {
            let mut token_changed = auth_token_proxy.receive_token_changed().await;
            while let Some(token) = token_changed.next().await {
                let token = token.get().await.expect("should have received token");
                send.send(token)
                    .expect("should have sent token to watchers");
            }
        };

        tokio::select! {
            _ = cancel_token.cancelled() => {
                println!("cancelled");
            }
            _ = token_updater => {
                println!("token updater");
            }
        }
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
        let cancel_token = CancellationToken::new();
        let orb_token = OrbToken::read(&cancel_token)
            .await
            .expect("should have token");

        // Test getting token
        let retrieved_token = orb_token.value().await?;
        assert_eq!(retrieved_token, test_manager.token().await);

        connection.close().await?;
        cancel_token.cancel();
        Ok(())
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_token_update() -> Result<()> {
        let (connection, test_manager) = setup_test_server().await?;

        // Create client
        let orb_token = OrbToken::read(&CancellationToken::new())
            .await
            .expect("should have token");

        // Get initial token
        let initial_token = orb_token.value().await?;
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
        let updated_token = orb_token.value().await?;
        assert_eq!(updated_token, "updated_token");

        connection.close().await?;
        Ok(())
    }
}
