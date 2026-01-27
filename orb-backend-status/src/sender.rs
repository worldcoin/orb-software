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
            let should_send_now = tokio::select! {
                _ = shutdown_token.cancelled() => break,

                // Periodic interval (30 seconds)
                _ = interval.tick() => true,

                // Let connectivity watcher do it's thing
                // It can trigger an urgent flag on WiFi SSID change
                // TODO: this should not be here. It makes 0 sense.
                // Manual SSID change tests feels slower without this
                // Probably they are flaky, but I keep this for now.
                // Need to think
                _ = connectivity_receiver.changed() => false,

                // Something urgent happened (reboot or SSID change)
                _ = backend_status.wait_for_urgent_send() => true,
            };

            let urgent_pending = backend_status.should_send_immediately();

            // TODO: also remove this when the waking of connectivity_receiver is removed
            if !should_send_now && !urgent_pending {
                // Woke up due to connectivity/hardware_states change but nothing urgent - just loop back
                continue;
            }

            // We want to send after this stage (interval ticked or urgent)
            // So we check if we are connected. If not connected, go into wait for connection loop
            let connected = connectivity_receiver.borrow().is_connected();
            if !connected {
                info!("not globally connected - waiting for connection");
                loop {
                    tokio::select! {
                        _ = shutdown_token.cancelled() => return,
                        _ = connectivity_receiver.changed() => {
                            if connectivity_receiver.borrow().is_connected() {
                                break;
                            }
                        }
                    }
                }
                info!("connection restored, proceeding with send");
            }

            let token = token_receiver.borrow().clone();
            if token.is_empty() {
                info!("auth token not available yet - skipping send");
                continue;
            }

            let snapshot = backend_status.snapshot();

            // It should be OK to send now, but sometimes
            // GlobalConnectivity does not fully guarantee that we can send
            // So we still have a backoff
            match self.send_snapshot(&snapshot, &token).await {
                Ok(_) => {
                    backend_status.clear_send_immediately();
                    backoff = self.min_backoff;
                    interval.reset();
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
