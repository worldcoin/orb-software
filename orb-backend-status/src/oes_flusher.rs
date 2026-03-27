use crate::backend::client::{self, StatusClient};
use crate::backend::types::OrbStatusApiV2;
use crate::collectors::oes::Event;
use std::time::Duration;
use tokio::time::{self};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, warn};

pub async fn run_oes_flush_loop(
    oes_rx: flume::Receiver<Event>,
    client: StatusClient,
    shutdown_token: CancellationToken,
) {
    let mut buffer: Vec<Event> = Vec::new();
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

                    if let Err(e) = flush_events(
                        &client,
                        &buffer,
                    ).await {
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

        maybe_flush(&client, &mut buffer).await;
    }
}

fn drain_available(rx: &flume::Receiver<Event>, buffer: &mut Vec<Event>) {
    while let Ok(event) = rx.try_recv() {
        buffer.push(event);
    }
}

const MAX_BATCH_EVENTS: usize = 100;

#[allow(clippy::too_many_arguments)]
async fn maybe_flush(client: &StatusClient, buffer: &mut Vec<Event>) {
    if buffer.is_empty() {
        return;
    }

    let batch_size = buffer.len().min(MAX_BATCH_EVENTS);
    let batch = &buffer[..batch_size];

    match flush_events(client, batch).await {
        Ok(()) => {
            debug!(count = batch_size, "OES flush successful");
            buffer.drain(..batch_size);
        }

        Err(e) => {
            error!(
                count = buffer.len(),
                "OES flush failed, events remain buffered: {e}",
            );
        }
    }
}

async fn flush_events(client: &StatusClient, events: &[Event]) -> eyre::Result<()> {
    let req = OrbStatusApiV2 {
        oes: Some(events.to_vec()),
        ..Default::default()
    };

    let res = match client.req(req).await {
        Err(client::Err::MissingAttestToken | client::Err::NoConnectivity) => {
            return Ok(());
        }

        Err(client::Err::Other(e)) => return Err(e),

        Ok(res) => res,
    };

    let status = res.status();
    if !status.is_success() {
        let body = res.text().await.unwrap_or_default();
        return Err(eyre::eyre!("OES flush error: {status} - {body}"));
    }

    Ok(())
}
