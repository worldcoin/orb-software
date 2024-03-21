//! Dbus interface definitions.

use crate::engine;
use crate::engine::Event;
use tokio::sync::mpsc;
use tracing::debug;
use zbus::interface;

/// Dbus interface object for OrbSignupState1.
#[derive(Debug)]
pub struct Interface {
    events: mpsc::UnboundedSender<Event>,
}

impl Interface {
    pub fn new(events: mpsc::UnboundedSender<Event>) -> Self {
        Self { events }
    }
}

#[interface(name = "org.worldcoin.OrbSignupState1")]
impl Interface {
    /// Forward events to UI engine by sending serialized engine::Event to the event channel.
    async fn orb_signup_state_event(&mut self, event: String) -> zbus::fdo::Result<()> {
        // parse event to engine::Event using json_serde
        debug!("received JSON event: {}", event);
        let event: engine::Event = serde_json::from_str(&event).map_err(|e| {
            zbus::fdo::Error::InvalidArgs(format!(
                "invalid event: failed to parse {}",
                e
            ))
        })?;
        debug!("received event: {:?}", event);
        self.events.send(event).map_err(|e| {
            zbus::fdo::Error::Failed(format!("failed to queue event: {}", e))
        })?;
        Ok(())
    }
}
