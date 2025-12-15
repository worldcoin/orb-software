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
}

impl BackendSender {
    pub fn new(client: StatusClient) -> Self {
        Self { client }
    }

    pub async fn send_snapshot(
        &self,
        snapshot: &CurrentStatus,
        token: &str,
    ) -> Result<()> {
        if token.is_empty() {
            info!("auth token not available yet - skipping send");
            return Ok(());
        }

        self.client.send_status(snapshot, token).await
    }
}

pub async fn run_loop(
    backend_status: crate::dbus::intf_impl::BackendStatusImpl,
    sender: BackendSender,
    token_receiver: watch::Receiver<String>,
    mut connectivity_receiver: watch::Receiver<GlobalConnectivity>,
    send_interval: std::time::Duration,
    shutdown_token: CancellationToken,
) {
    let mut backoff = std::time::Duration::from_secs(1);
    let max_backoff = std::time::Duration::from_secs(30);

    let mut interval = time::interval(send_interval);
    interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

    loop {
        let urgent = backend_status.should_send_immediately();
        let connected = connectivity_receiver.borrow().is_connected();
        let should_send_now = if urgent {
            // Any update that sets the urgent flag should trigger an immediate send.
            true
        } else {
            // Otherwise we only send on the periodic tick.
            tokio::select! {
                _ = shutdown_token.cancelled() => break,
                _ = interval.tick() => true,
                _ = backend_status.wait_for_change() => false,
                _ = connectivity_receiver.changed() => false,
            }
        };

        if !should_send_now {
            continue;
        }

        if !connected {
            info!("not globally connected - skipping send");
            continue;
        }

        let token = token_receiver.borrow().clone();
        let snapshot = backend_status.snapshot();

        match sender.send_snapshot(&snapshot, &token).await {
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


