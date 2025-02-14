use color_eyre::eyre::{Result, WrapErr};
use orb_attest_dbus::AuthTokenManagerProxy;
use std::sync::Arc;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use tracing::debug;
use zbus::export::futures_util::StreamExt;
use zbus::Connection;

use crate::OrbInfoError;

#[derive(Debug, Clone)]
pub struct TokenTaskHandle {
    pub token_receiver: watch::Receiver<String>,
    pub join_handle: Arc<tokio::task::JoinHandle<Result<()>>>,
}

impl TokenTaskHandle {
    pub async fn spawn(
        connection: &Connection,
        cancel_token: &CancellationToken,
    ) -> Result<Self, OrbInfoError> {
        let (join_handle, token_receiver) =
            Self::setup_dbus(connection, cancel_token).await?;
        Ok(TokenTaskHandle {
            token_receiver,
            join_handle: Arc::new(join_handle),
        })
    }

    pub fn value(&self) -> String {
        self.token_receiver.borrow().to_owned()
    }

    async fn setup_dbus(
        connection: &Connection,
        cancel_token: &CancellationToken,
    ) -> Result<
        (tokio::task::JoinHandle<Result<()>>, watch::Receiver<String>),
        OrbInfoError,
    > {
        let auth_token_proxy = AuthTokenManagerProxy::new(connection)
            .await
            .map_err(OrbInfoError::ZbusErr)?;

        let initial_value = auth_token_proxy.token().await?;
        let (send, recv) = watch::channel(initial_value);
        let auth_token_proxy = auth_token_proxy.clone();
        let cancel_token = cancel_token.clone();
        let join_handle =
            tokio::task::spawn(Self::task_inner(auth_token_proxy, send, cancel_token));
        Ok((join_handle, recv))
    }

    async fn task_inner(
        auth_token_proxy: AuthTokenManagerProxy<'_>,
        send: watch::Sender<String>,
        cancel_token: CancellationToken,
    ) -> Result<()> {
        let token_updater_fut = async move {
            let mut token_changed = auth_token_proxy.receive_token_changed().await;
            while let Some(token) = token_changed.next().await {
                let token = token
                    .get()
                    .await
                    .wrap_err("failed to get token over dbus")?;
                if send.send(token).is_err() {
                    // normal for this to fail if for example, all watchers are dropped
                    return Ok(());
                }
            }
            Ok(())
        };

        tokio::select! {
            _ = cancel_token.cancelled() => {
                debug!("Cancelling token watcher task");
                Ok(())
            }
            result = token_updater_fut => result,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orb_attest_dbus::AuthTokenManagerT;
    use std::{
        sync::{Arc, Mutex},
        time::Duration,
    };
    use zbus::ConnectionBuilder;

    type AuthTokenManagerIface = orb_attest_dbus::AuthTokenManager<Mocked>;

    #[derive(Clone, Debug)]
    struct Mocked {
        token: Arc<Mutex<String>>,
    }

    // Note how we are simply implementing a trait from orb-attest-dbus instead of creating an entirely new struct with zbus macros.
    // This ensures that the function signatures all match up and we get good compile errors and LSP support.
    impl AuthTokenManagerT for Mocked {
        fn token(&self) -> zbus::fdo::Result<String> {
            Ok(self.token.lock().unwrap().clone())
        }

        fn force_token_refresh(&mut self, _ctxt: zbus::SignalContext<'_>) {
            // no-op
        }
    }

    // using `dbus_launch` ensures that all tests use their own isolated dbus, and that they can't influence each other.
    async fn start_dbus_daemon() -> dbus_launch::Daemon {
        tokio::task::spawn_blocking(|| {
            dbus_launch::Launcher::daemon()
                .launch()
                .expect("failed to launch dbus-daemon")
        })
        .await
        .expect("task panicked")
    }

    async fn setup_test_server() -> Result<(Connection, dbus_launch::Daemon, Mocked)> {
        let token = Arc::new(Mutex::new("test_token".to_string()));
        let mock_manager = Mocked { token };
        let daemon = start_dbus_daemon().await;

        let connection = ConnectionBuilder::address(daemon.address())?
            .name("org.worldcoin.AuthTokenManager1")?
            .serve_at(
                "/org/worldcoin/AuthTokenManager1",
                orb_attest_dbus::AuthTokenManager(mock_manager.clone()),
            )?
            .build()
            .await?;

        Ok((connection, daemon, mock_manager))
    }

    #[tokio::test]
    async fn test_get_orb_token() -> Result<()> {
        let (connection, _, mock_manager) = setup_test_server().await?;

        // Create client
        let cancel_token = CancellationToken::new();
        let orb_token = TokenTaskHandle::spawn(&connection, &cancel_token)
            .await
            .expect("should have token");

        // Test getting token
        let retrieved_token = orb_token.value();
        assert_eq!(retrieved_token, mock_manager.token().unwrap());

        connection.close().await?;
        cancel_token.cancel();
        Ok(())
    }

    #[tokio::test]
    async fn test_token_update() -> Result<()> {
        let (connection, _, mock_manager) = setup_test_server().await?;
        let object_server = connection.object_server();
        let iface_ref = object_server
            .interface::<_, AuthTokenManagerIface>("/org/worldcoin/AuthTokenManager1")
            .await
            .wrap_err(
                "failed to get reference to AuthTokenManager1 from object server",
            )?;

        // Create client
        let orb_token = TokenTaskHandle::spawn(&connection, &CancellationToken::new())
            .await
            .expect("should have token");

        // Get initial token
        let initial_token = orb_token.value();
        assert_eq!(initial_token, "test_token");

        // Update token via proxy
        {
            let mut token_guard = mock_manager.token.lock().unwrap();
            *token_guard = "updated_token".to_string();
        }
        iface_ref
            .get_mut()
            .await
            .token_changed(iface_ref.signal_context())
            .await
            .wrap_err("failed to send token_changed signal")?;

        // Verify updated token is retrieved
        tokio::time::sleep(Duration::from_millis(100)).await;
        let updated_token = orb_token.value();
        assert_eq!(updated_token, "updated_token");

        // connection.close().await?;
        Ok(())
    }
}
