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
        token_receiver: watch::Receiver<String>,
        mut connectivity_receiver: watch::Receiver<GlobalConnectivity>,
        shutdown_token: CancellationToken,
    ) {
        let mut backoff = self.min_backoff;
        let max_backoff = self.max_backoff;

        let mut interval = time::interval(self.interval);
        interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                _ = shutdown_token.cancelled() => break,
                _ = interval.tick() => (),
                _ = connectivity_receiver.changed() => (),
                _ = backend_status.wait_for_urgent_change() => (),
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
