use crate::backend::status::StatusClient;
use crate::dbus::intf_impl::CurrentStatus;
use color_eyre::eyre::Result;
use tokio::sync::watch;
use tokio::time::{self, Instant};
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
    send_interval: std::time::Duration,
    shutdown_token: CancellationToken,
) {
    let mut last_send = Instant::now() - send_interval;
    let mut backoff = std::time::Duration::from_secs(1);
    let max_backoff = std::time::Duration::from_secs(30);

    loop {
        let changed = backend_status.changed();
        let urgent = backend_status.should_send_immediately();
        let interval_elapsed = last_send.elapsed() >= send_interval;

        if changed && (urgent || interval_elapsed) {
            let token = token_receiver.borrow().clone();
            let snapshot = backend_status.snapshot();

            match sender.send_snapshot(&snapshot, &token).await {
                Ok(_) => {
                    backend_status.clear_changed();
                    backend_status.clear_send_immediately();
                    last_send = Instant::now();
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
            }

            continue;
        }

        // Nothing to do right now: wait for a change or for the next send deadline.
        let deadline = last_send + send_interval;
        let sleep_until_deadline = time::sleep_until(deadline);
        tokio::pin!(sleep_until_deadline);

        tokio::select! {
            _ = shutdown_token.cancelled() => break,
            _ = backend_status.wait_for_change() => {}
            _ = &mut sleep_until_deadline => {}
        }
    }
}


