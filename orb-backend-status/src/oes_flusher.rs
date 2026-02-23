use crate::backend::types::OrbStatusApiV2;
use crate::collectors::connectivity::GlobalConnectivity;
use crate::collectors::oes::Event;
use chrono::Utc;
use orb_info::OrbId;
use reqwest::Url;
use std::time::Duration;
use tokio::sync::watch;
use tokio::time::{self, Instant};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

pub async fn run_oes_flush_loop(
    oes_rx: flume::Receiver<Event>,
    endpoint: Url,
    orb_id: OrbId,
    mut token_receiver: watch::Receiver<String>,
    connectivity_receiver: watch::Receiver<GlobalConnectivity>,
    shutdown_token: CancellationToken,
) {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("failed to build OES reqwest client");

    // Wait for a non-empty auth token before entering the main loop
    let token = loop {
        let current = token_receiver.borrow().clone();
        if !current.is_empty() {
            break current;
        }
        tokio::select! {
            _ = shutdown_token.cancelled() => {
                info!("Shutdown before OES token received");

                return;
            }
            result = token_receiver.changed() => {
                if result.is_err() {
                    warn!("OES token channel closed before token received");

                    return;
                }
            }
        }
    };
    debug!("OES flusher received auth token");

    let mut buffer: Vec<Event> = Vec::new();
    let mut last_flush = Instant::now() - Duration::from_secs(1);
    let mut backoff = Duration::from_secs(1);
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
                        &endpoint,
                        &orb_id,
                        &token,
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

        maybe_flush(
            &client,
            &endpoint,
            &orb_id,
            &token,
            &mut buffer,
            &mut last_flush,
            &mut backoff,
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

const MAX_BACKOFF: Duration = Duration::from_secs(60);
const MAX_BATCH_EVENTS: usize = 100;

#[allow(clippy::too_many_arguments)]
async fn maybe_flush(
    client: &reqwest::Client,
    endpoint: &Url,
    orb_id: &OrbId,
    token: &str,
    buffer: &mut Vec<Event>,
    last_flush: &mut Instant,
    backoff: &mut Duration,
    connectivity_receiver: &watch::Receiver<GlobalConnectivity>,
) {
    if buffer.is_empty() {
        return;
    }

    if last_flush.elapsed() < *backoff {
        return;
    }

    if !connectivity_receiver.borrow().is_connected() {
        debug!(count = buffer.len(), "Orb offline, skipping OES flush");

        return;
    }

    let batch_size = buffer.len().min(MAX_BATCH_EVENTS);
    let batch = &buffer[..batch_size];

    match flush_events(client, endpoint, orb_id, token, batch).await {
        Ok(()) => {
            debug!(count = batch_size, "OES flush successful");
            buffer.drain(..batch_size);
            *last_flush = Instant::now();
            *backoff = Duration::from_secs(1);
        }
        Err(e) => {
            error!(
                count = buffer.len(),
                "OES flush failed, events remain buffered: {e}",
            );
            *last_flush = Instant::now();
            *backoff = (*backoff * 2).min(MAX_BACKOFF);
        }
    }
}

async fn flush_events(
    client: &reqwest::Client,
    endpoint: &Url,
    orb_id: &OrbId,
    token: &str,
    events: &[Event],
) -> eyre::Result<()> {
    let request = OrbStatusApiV2 {
        oes: Some(events.to_vec()),
        timestamp: Utc::now(),
        ..Default::default()
    };

    let response = client
        .post(endpoint.clone())
        .json(&request)
        .basic_auth(orb_id.to_string(), Some(token))
        .send()
        .await?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();

        return Err(eyre::eyre!("OES flush error: {status} - {body}"));
    }

    Ok(())
}

