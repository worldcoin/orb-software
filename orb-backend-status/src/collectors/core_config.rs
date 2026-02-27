use crate::dbus::intf_impl::BackendStatusImpl;
use color_eyre::{eyre::eyre, Result};
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tracing::{trace, warn};
use zenorb::zenoh;

/// The zenoh key expression for orb-core's published config.
/// orb-core publishes via `Zenorb::put("config", ...)` with namespace "core",
/// resulting in the full key `{orb_id}/core/config`.
/// The receiver auto-prefixes `{orb_id}/`, so we subscribe to `core/config`.
const CORE_CONFIG_KEY_EXPR: &str = "core/config";

pub struct CoreConfigWatcher {
    pub task: tokio::task::JoinHandle<()>,
}

pub async fn spawn_watcher(
    zsession: &zenorb::Zenorb,
    backend_status: BackendStatusImpl,
    shutdown_token: CancellationToken,
) -> Result<CoreConfigWatcher> {
    let mut tasks = zsession
        .receiver(backend_status)
        .querying_subscriber(
            CORE_CONFIG_KEY_EXPR,
            Duration::from_secs(1),
            handle_core_config_event,
        )
        .run()
        .await?;

    let subscriber_task = tasks
        .pop()
        .ok_or_else(|| eyre!("expected subscriber task"))?;

    let task = tokio::spawn(async move {
        shutdown_token.cancelled().await;
        subscriber_task.abort();
    });

    Ok(CoreConfigWatcher { task })
}

async fn handle_core_config_event(
    backend_status: BackendStatusImpl,
    sample: zenoh::sample::Sample,
) -> Result<()> {
    let key = sample.key_expr().to_string();

    let payload_str = match sample.payload().try_to_string() {
        Ok(p) => p,
        Err(e) => {
            warn!("Failed to convert core config payload to string for key {key}: {e}");

            return Ok(());
        }
    };

    let config: serde_json::Value = match serde_json::from_str(payload_str.as_ref()) {
        Ok(v) => v,
        Err(e) => {
            warn!("Failed to parse core config JSON for key {key}: {e}");

            return Ok(());
        }
    };

    trace!("Received core config: {config}");
    backend_status.update_core_config(Some(config));

    Ok(())
}
