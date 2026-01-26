use crate::dbus::intf_impl::BackendStatusImpl;
use color_eyre::{eyre::eyre, Result};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tracing::{debug, trace, warn};

/// The zenoh key expression for hardware status.
pub const HARDWARE_STATUS_KEY_EXPR: &str = "hardware/status/**";

/// Hardware state payload from zenoh.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct HardwareState {
    /// The status of the hardware component (e.g., "success", "failure").
    pub status: String,
    /// A message describing the current state (e.g., "corded", "disconnected").
    pub message: String,
}

pub struct HardwareStatesWatcher {
    pub task: tokio::task::JoinHandle<()>,
}

/// Spawn a hardware states watcher that subscribes to zenoh hardware/status/** topic.
pub async fn spawn_watcher(
    zsession: &zenorb::Zenorb,
    backend_status: BackendStatusImpl,
    shutdown_token: CancellationToken,
) -> Result<HardwareStatesWatcher> {
    let ctx = WatcherCtx {
        states: Arc::new(Mutex::new(HashMap::new())),
        backend_status,
    };

    let mut tasks = zsession
        .receiver(ctx)
        .querying_subscriber(
            HARDWARE_STATUS_KEY_EXPR,
            Duration::from_millis(100),
            handle_hardware_state_event,
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

    Ok(HardwareStatesWatcher { task })
}

#[derive(Clone)]
struct WatcherCtx {
    states: Arc<Mutex<HashMap<String, HardwareState>>>,
    backend_status: BackendStatusImpl,
}

async fn handle_hardware_state_event(
    ctx: WatcherCtx,
    sample: zenoh::sample::Sample,
) -> Result<()> {
    let key = sample.key_expr().to_string();
    let component_name = extract_component_name(&key);

    let payload = match sample.payload().try_to_string() {
        Ok(p) => p,
        Err(e) => {
            warn!("Failed to convert payload to string for key {key}: {e}");
            return Ok(());
        }
    };

    let state: HardwareState = match serde_json::from_str(payload.as_ref()) {
        Ok(s) => s,
        Err(e) => {
            warn!("Failed to parse HardwareState for key {key}: {e}");
            return Ok(());
        }
    };

    trace!("Received hardware state for {component_name}: {:?}", state);

    let mut states = ctx.states.lock().await;
    let prev = states.get(&component_name);
    if prev != Some(&state) {
        debug!(
            "hardware state changed for {component_name}: {:?} -> {:?}",
            prev, state
        );
    }
    states.insert(component_name, state);

    // Update the backend status with the new hardware states
    ctx.backend_status.update_hardware_states(states.clone());

    Ok(())
}

/// Extracts the component name from a zenoh key.
///
/// For example, "bfd00a01/hardware/status/pwr_supply" -> "pwr_supply"
fn extract_component_name(key: &str) -> String {
    key.rsplit('/').next().unwrap_or(key).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_component_name() {
        assert_eq!(
            extract_component_name("bfd00a01/hardware/status/pwr_supply"),
            "pwr_supply"
        );
        assert_eq!(extract_component_name("hardware/status/battery"), "battery");
        assert_eq!(extract_component_name("single"), "single");
    }

    #[test]
    fn test_hardware_state_deserialize() {
        let json = r#"{"status": "success", "message": "corded"}"#;
        let state: HardwareState = serde_json::from_str(json).unwrap();
        assert_eq!(state.status, "success");
        assert_eq!(state.message, "corded");
    }
}
