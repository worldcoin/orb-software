use crate::backend::oes::OesClient;
use crate::collectors::connectivity::GlobalConnectivity;
use crate::collectors::oes::Event;
use std::time::Duration;
use tokio::sync::watch;
use tokio::time::{self, Instant};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, warn};

pub async fn run_oes_flush_loop(
    oes_rx: flume::Receiver<Event>,
    client: OesClient,
    connectivity_receiver: watch::Receiver<GlobalConnectivity>,
    shutdown_token: CancellationToken,
) {
    let mut buffer: Vec<Event> = Vec::new();
    let mut last_flush = Instant::now() - Duration::from_secs(1);
    let mut interval = time::interval(Duration::from_secs(1));
    interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            _ = shutdown_token.cancelled() => {
                if !buffer.is_empty() {
                    debug!(
                        count = buffer.len(),
                        "Shutdown: attempting final OES flush",
                    );
                    if let Err(e) = client.send_events(&buffer).await {
                        warn!("Final OES flush failed: {e}");
                    }
                }

                break;
            }

            result = oes_rx.recv_async() => {
                match result {
                    Ok(event) => {
                        buffer.push(event);
                        drain_available(&oes_rx, &mut buffer);
                    }
                    Err(_) => {
                        debug!("OES channel closed, exiting flush loop");

                        break;
                    }
                }
            }

            _ = interval.tick() => {}
        }

        maybe_flush(
            &client,
            &mut buffer,
            &mut last_flush,
            &connectivity_receiver,
        )
        .await;
    }
}

fn drain_available(rx: &flume::Receiver<Event>, buffer: &mut Vec<Event>) {
    while let Ok(event) = rx.try_recv() {
        buffer.push(event);
    }
}

async fn maybe_flush(
    client: &OesClient,
    buffer: &mut Vec<Event>,
    last_flush: &mut Instant,
    connectivity_receiver: &watch::Receiver<GlobalConnectivity>,
) {
    if buffer.is_empty() {
        return;
    }

    if last_flush.elapsed() < Duration::from_secs(1) {
        return;
    }

    if !connectivity_receiver.borrow().is_connected() {
        debug!(count = buffer.len(), "Orb offline, skipping OES flush",);

        return;
    }

    match client.send_events(buffer).await {
        Ok(()) => {
            debug!(count = buffer.len(), "OES flush successful");
            buffer.clear();
            *last_flush = Instant::now();
        }
        Err(e) => {
            error!(
                count = buffer.len(),
                "OES flush failed, events remain buffered: {e}",
            );
        }
    }
}
