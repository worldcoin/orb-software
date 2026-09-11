pub mod backend;
pub mod collectors;
pub mod dbus;
pub mod orb_event_stream;
pub mod sender;

use crate::{
    backend::boot_id::orb_boot_id,
    orb_event_stream::{Event, OrbEventStream, Payload},
    sender::BackendSender,
};
use backend::client::StatusClient;
use chrono::Utc;
use color_eyre::eyre::Result;
use orb_build_info::{make_build_info, BuildInfo};
use orb_dogd::MetricEmitter;
use orb_info::{OrbId, OrbJabilId, OrbName};
use reqwest::Url;
use std::{path::PathBuf, time::Duration};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};
use zenorb::Zenorb as ZSession;

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
pub async fn program(
    metrics: impl MetricEmitter,
    collector_config: collectors::Config,
    zsession: &ZSession,
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
    shutdown_token: CancellationToken,
) -> Result<()> {
    info!("Starting backend-status, endpoint: {endpoint}, orb_id: {orb_id}, orb_name: {orb_name}, orb_jabil_id: {orb_jabil_id}");

    let procfs = procfs.into();
    let boot_id = orb_boot_id(&procfs)
        .await
        .inspect_err(|e| warn!("failed to read boot-id: {e:?}"))
        .ok();

    let (collectors, token_receiver, connectivity_receiver) =
        collectors::Collectors::new(collector_config, shutdown_token.clone()).await?;

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
        .attest_token_rx(token_receiver)
        .connectivity_rx(connectivity_receiver.clone())
        .build();

    let mut tasks = collectors.spawn_reporters(procfs, shutdown_token.clone());

    let oes = OrbEventStream::start(status_client.clone(), shutdown_token.clone());
    if let Some(boot_id) = boot_id
        && let Err(e) = oes.ingest(boot_id_payload(boot_id)?)
    {
        warn!("failed to cache boot-id OES event: {e:?}");
    }

    let mut zenorb_tasks = collectors.subscribe(zsession, oes.clone()).await?;

    zenorb_tasks.extend(
        zsession
            .receiver(oes.clone())
            .subscriber(
                orb_event_stream::KEY_EXPR,
                orb_event_stream::collector::handler,
            )
            .run()
            .await?,
    );

    let sender = BackendSender::new(status_client.clone(), oes, sender_interval);
    sender.run_loop(collectors, shutdown_token.clone()).await;

    // Spawn a single shutdown task for all zenorb subscribers
    let shutdown = shutdown_token.clone();
    tasks.push(tokio::spawn(async move {
        shutdown.cancelled().await;
        for task in zenorb_tasks {
            task.abort();
        }
    }));

    for task in tasks {
        task.abort();
    }

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
