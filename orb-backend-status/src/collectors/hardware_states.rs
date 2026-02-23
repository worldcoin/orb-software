use super::ZenorbCtx;
use color_eyre::Result;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{trace, warn};
use zenorb::{zenoh, Receiver};

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

pub(crate) fn register(receiver: Receiver<'_, ZenorbCtx>) -> Receiver<'_, ZenorbCtx> {
    receiver.querying_subscriber(
        HARDWARE_STATUS_KEY_EXPR,
        Duration::from_millis(100),
        handle_hardware_state_event,
    )
}

async fn handle_hardware_state_event(
    ctx: ZenorbCtx,
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

    let mut states = ctx.hardware_states.lock().await;
    states.insert(component_name, state);

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
    fn test_extract_component_name_empty() {
        assert_eq!(extract_component_name(""), "");
    }

    #[test]
    fn test_extract_component_name_with_trailing_slash() {
        assert_eq!(extract_component_name("bfd00a01/hardware/status/"), "");
    }
}
