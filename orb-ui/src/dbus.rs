//! Dbus interface definitions.

use crate::engine;
use crate::engine::Event;
use tokio::sync::mpsc;
use zbus::interface;

/// Dbus interface object for OrbUiState1.
#[derive(Debug)]
pub struct Interface {
    events: mpsc::UnboundedSender<Event>,
}

impl Interface {
    pub fn new(events: mpsc::UnboundedSender<Event>) -> Self {
        Self { events }
    }
}

#[interface(name = "org.worldcoin.OrbUiState1")]
impl Interface {
    /// Forward events to UI engine by sending serialized engine::Event to the event channel.
    async fn orb_signup_state_event(&mut self, event: String) -> zbus::fdo::Result<()> {
        // parse event to engine::Event using json_serde
        tracing::debug!("received JSON event: {}", event);
        let event: engine::Event = serde_json::from_str(&event).map_err(|e| {
            zbus::fdo::Error::InvalidArgs(format!(
                "invalid event: failed to parse {}",
                e
            ))
        })?;
        self.events.send(event).map_err(|e| {
            zbus::fdo::Error::Failed(format!("failed to queue event: {}", e))
        })?;
        Ok(())
    }
}
