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
use tracing::{debug, error, warn};

pub async fn run_oes_flush_loop(
    oes_rx: flume::Receiver<Event>,
    client: reqwest::Client,
    endpoint: Url,
    orb_id: OrbId,
    token_receiver: watch::Receiver<String>,
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
                    if let Err(e) = flush_events(
                        &client,
                        &endpoint,
                        &orb_id,
                        &token_receiver,
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
            &token_receiver,
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
    client: &reqwest::Client,
    endpoint: &Url,
    orb_id: &OrbId,
    token_receiver: &watch::Receiver<String>,
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
        debug!(count = buffer.len(), "Orb offline, skipping OES flush");

        return;
    }

    match flush_events(client, endpoint, orb_id, token_receiver, buffer).await {
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

async fn flush_events(
    client: &reqwest::Client,
    endpoint: &Url,
    orb_id: &OrbId,
    token_receiver: &watch::Receiver<String>,
    events: &[Event],
) -> eyre::Result<()> {
    let token = token_receiver.borrow().clone();
    if token.is_empty() {
        return Err(eyre::eyre!("auth token not available yet"));
    }

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
