use orb_info::TokenTaskHandle;
use std::{sync::Arc, time::Duration};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use tracing::error;
use zbus::Connection;

pub struct TokenWatcher;

impl TokenWatcher {
    /// Spawn a token watcher that monitors the AuthTokenManager D-Bus service.
    ///
    /// Fetches the initial token before returning to ensure it's available
    /// immediately to the sender.
    pub async fn spawn(
        connection: Connection,
        shutdown_token: CancellationToken,
    ) -> watch::Receiver<String> {
        // Try to get initial token before spawning background task
        let initial_token = match TokenTaskHandle::spawn(&connection, &shutdown_token).await {
            Ok(task) => task.token_recv.borrow().clone(),
            Err(e) => {
                error!("failed initial token fetch (will retry in background): {e:?}");
                String::new()
            }
        };

        let (token_sender, token_receiver) = watch::channel(initial_token);

        tokio::spawn(async move {
            let mut backoff = Duration::from_secs(1);

            loop {
                if shutdown_token.is_cancelled() {
                    break;
                }

                let token_task = match TokenTaskHandle::spawn(&connection, &shutdown_token).await {
                    Ok(task) => Arc::new(task),
                    Err(e) => {
                        error!("failed to spawn token watcher task (will retry): {e:?}");
                        tokio::select! {
                            _ = shutdown_token.cancelled() => break,
                            () = tokio::time::sleep(backoff) => {}
                        }
                        backoff = (backoff * 2).min(Duration::from_secs(30));
                        continue;
                    }
                };
                backoff = Duration::from_secs(1);

                let mut token_recv = token_task.token_recv.clone();

                // Forward the current token immediately
                let current_token = token_recv.borrow_and_update().clone();
                let _ = token_sender.send(current_token);

                loop {
                    tokio::select! {
                        _ = shutdown_token.cancelled() => return,
                        changed = token_recv.changed() => {
                            if changed.is_err() {
                                break;
                            }

                            let token = token_recv.borrow().clone();
                            let _ = token_sender.send(token);
                        }
                    }
                }
            }
        });

        token_receiver
    }
}
