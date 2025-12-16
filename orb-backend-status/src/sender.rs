use std::time::Duration;

use crate::backend::status::StatusClient;
use crate::collectors::connectivity::GlobalConnectivity;
use crate::dbus::intf_impl::CurrentStatus;
use color_eyre::eyre::Result;
use tokio::sync::watch;
use tokio::time::{self};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

#[derive(Clone)]
pub struct BackendSender {
    client: StatusClient,
    interval: Duration,
    min_backoff: Duration,
    max_backoff: Duration,
}

impl BackendSender {
    pub fn new(
        client: StatusClient,
        interval: Duration,
        min_backoff: Duration,
        max_backoff: Duration,
    ) -> Self {
        Self {
            client,
            interval,
            min_backoff,
            max_backoff,
        }
    }

    pub async fn send_snapshot(
        &self,
        snapshot: &CurrentStatus,
        token: &str,
    ) -> Result<()> {
        self.client.send_status(snapshot, token).await
    }

    pub async fn run_loop(
        self,
        backend_status: crate::dbus::intf_impl::BackendStatusImpl,
        mut token_receiver: watch::Receiver<String>,
        mut connectivity_receiver: watch::Receiver<GlobalConnectivity>,
        shutdown_token: CancellationToken,
    ) {
        let mut backoff = self.min_backoff;
        let max_backoff = self.max_backoff;

        let mut interval = time::interval(self.interval);
        interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

        loop {
            if shutdown_token.is_cancelled() {
                break;
            }

            let urgent = backend_status.should_send_immediately();

            // Urgent means: send immediately *when possible* (still gated on connectivity + token).
            // If we can't send yet, wait for the relevant condition(s) to change rather than spinning.
            let should_send_now = if urgent {
                let connected = connectivity_receiver.borrow().is_connected();
                let token_present = !token_receiver.borrow().is_empty();
                if connected && token_present {
                    true
                } else {
                    tokio::select! {
                        _ = shutdown_token.cancelled() => break,
                        _ = token_receiver.changed() => false,
                        _ = connectivity_receiver.changed() => false,
                        _ = backend_status.wait_for_change() => false,
                    }
                }
            } else {
                // Otherwise we only send on the periodic tick
                tokio::select! {
                    _ = shutdown_token.cancelled() => break,
                    _ = interval.tick() => true,
                    _ = backend_status.wait_for_change() => false,
                    _ = token_receiver.changed() => false,
                    _ = connectivity_receiver.changed() => false,
                }
            };

            if !should_send_now {
                continue;
            }

            let connected = connectivity_receiver.borrow().is_connected();
            if !connected {
                info!("not globally connected - skipping send");
                continue;
            }

            let token = token_receiver.borrow().clone();
            if token.is_empty() {
                info!("auth token not available yet - skipping send");
                continue;
            }

            let snapshot = backend_status.snapshot();

            match self.send_snapshot(&snapshot, &token).await {
                Ok(_) => {
                    backend_status.clear_changed();
                    backend_status.clear_send_immediately();
                    backoff = std::time::Duration::from_secs(1);
                }
                Err(e) => {
                    error!("failed to send status (will backoff): {e:?}");
                    tokio::select! {
                        _ = shutdown_token.cancelled() => break,
                        () = time::sleep(backoff) => {}
                    }
                    backoff = (backoff * 2).min(max_backoff);
                }
            };
        }
    }
}
