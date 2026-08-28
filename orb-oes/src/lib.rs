mod boot_id;
mod event_server;
mod flusher;
mod sender;
mod status_client;
pub mod stream;
mod token_watcher;

use boot_id::orb_boot_id;
use chrono::Utc;
use color_eyre::eyre::{eyre, Result};
use orb_build_info::{make_build_info, BuildInfo};
use orb_dogd::MetricEmitter;
use orb_info::{OrbId, OrbJabilId, OrbName};
use reqwest::Url;
use status_client::StatusClient;
use std::{path::PathBuf, time::Duration};
use stream::{Event, OrbEventStream, Payload};
use tracing::{info, warn};

pub const BUILD_INFO: BuildInfo = make_build_info!();
const BOOT_ID_EVENT_NAME: &str = "system/boot_id";

fn boot_id_payload(boot_id: String) -> Result<Payload> {
    Ok(Payload {
        headers: oes::Headers::default().mode(oes::Mode::CacheOnly),
        event: Event {
            name: BOOT_ID_EVENT_NAME.to_string(),
            created_at: Utc::now().timestamp_millis(),
            payload: Some(serde_json::to_value(oes::BootIdEvent { boot_id })?),
        },
    })
}

#[bon::builder(finish_fn = run)]
pub fn program(
    metrics: impl MetricEmitter + Clone,
    endpoint: Url,
    orb_os_version: String,
    orb_id: OrbId,
    orb_name: OrbName,
    orb_jabil_id: OrbJabilId,
    sender_interval: Duration,
    req_timeout: Duration,
    req_min_retry_interval: Duration,
    req_max_retry_interval: Duration,
    procfs: impl Into<PathBuf>,
    shutdown_rx: flume::Receiver<()>,
) -> Result<()> {
    info!("Starting oes, endpoint: {endpoint}, orb_id: {orb_id}, orb_name: {orb_name}, orb_jabil_id: {orb_jabil_id}");

    let procfs = procfs.into();
    let boot_id = orb_boot_id(&procfs)
        .inspect_err(|e| warn!("failed to read boot-id: {e:?}"))
        .ok();

    rsbinder::ProcessState::init_default()
        .map_err(|e| eyre!("failed to initialize binder ProcessState: {e}"))?;
    // Spawns its own dedicated thread to service incoming binder calls; the
    // calling thread does not need to (and must not, to stay non-blocking)
    // join the pool itself.
    rsbinder::ProcessState::start_thread_pool();

    let (token, token_handle) = token_watcher::spawn(shutdown_rx.clone());

    let status_client = StatusClient::builder()
        .metrics(metrics)
        .orb_id(orb_id)
        .orb_name(orb_name)
        .jabil_id(orb_jabil_id)
        .orb_os_version(orb_os_version)
        .endpoint(endpoint)
        .req_timeout(req_timeout)
        .min_req_retry_interval(req_min_retry_interval)
        .max_req_retry_interval(req_max_retry_interval)
        .token(token)
        .build()?;

    let (oes, flusher_handle) =
        OrbEventStream::start(status_client.clone(), shutdown_rx.clone());
    if let Some(boot_id) = boot_id
        && let Err(e) = oes.ingest(boot_id_payload(boot_id)?)
    {
        warn!("failed to cache boot-id OES event: {e:?}");
    }

    event_server::register(oes.clone())
        .map_err(|e| eyre!("failed to register OES binder service: {e}"))?;

    let sender_handle = std::thread::spawn({
        let shutdown_rx = shutdown_rx.clone();
        move || {
            sender::run_cached_snapshot_loop(
                status_client,
                oes,
                sender_interval,
                shutdown_rx,
            )
        }
    });

    // Block until the shutdown channel is closed (see main.rs's signal
    // thread), then wait for every other loop to wind down (the flusher
    // does a final best-effort flush of any buffered events on shutdown).
    let _ = shutdown_rx.recv();

    let _ = token_handle.join();
    let _ = flusher_handle.join();
    let _ = sender_handle.join();

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_boot_id_payload_uses_cached_system_event() {
        let payload =
            boot_id_payload("16e16562-856b-4a20-9b46-4574a9be1d19".to_string())
                .unwrap();

        assert_eq!(payload.headers.mode, oes::Mode::CacheOnly);
        assert_eq!(payload.event.name, BOOT_ID_EVENT_NAME);
        assert_eq!(
            payload.event.payload,
            Some(serde_json::json!({
                "boot_id": "16e16562-856b-4a20-9b46-4574a9be1d19"
            }))
        );
    }
}
