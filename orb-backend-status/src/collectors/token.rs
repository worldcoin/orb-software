use orb_info::TokenTaskHandle;
use std::{sync::Arc, time::Duration};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use tracing::error;
use zbus::Connection;

pub struct TokenWatcher;

impl TokenWatcher {
    pub fn spawn(
        connection: Connection,
        shutdown_token: CancellationToken,
    ) -> watch::Receiver<String> {
        let (token_sender, token_receiver) = watch::channel(String::new());

        tokio::spawn(async move {
            let mut backoff = Duration::from_secs(1);

            loop {
                if shutdown_token.is_cancelled() {
                    break;
                }

                let token_task = match TokenTaskHandle::spawn(
                    &connection,
                    &shutdown_token,
                )
                .await
                {
                    Ok(task) => Arc::new(task),
                    Err(e) => {
                        error!(
                            "failed to spawn token watcher task (will retry): {e:?}"
                        );
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
                loop {
                    tokio::select! {
                        _ = shutdown_token.cancelled() => return,
                        changed = token_recv.changed() => {
                            if changed.is_err() {
                                // token publisher went away; try to re-establish
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
