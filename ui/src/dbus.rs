//! Dbus interface definitions.

use crate::engine::RxEvent;
use tokio::sync::mpsc;
use zbus::{interface, proxy};

/// Dbus interface object for OrbUiState1.
#[derive(Debug)]
pub struct InboundInterface {
    events: mpsc::UnboundedSender<RxEvent>,
}

impl InboundInterface {
    pub fn new(events: mpsc::UnboundedSender<RxEvent>) -> Self {
        Self { events }
    }
}

#[interface(name = "org.worldcoin.OrbUiState1")]
impl InboundInterface {
    /// Forward events to UI engine by sending serialized engine::TxEvent to the event channel.
    async fn orb_signup_state_event(&mut self, event: String) -> zbus::fdo::Result<()> {
        // parse serialized event
        tracing::trace!("received JSON event: {}", event);
        let event: RxEvent = serde_json::from_str(&event).map_err(|e| {
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

#[proxy(
    default_service = "org.worldcoin.OrbUserEvent1",
    default_path = "/org/worldcoin/OrbUserEvent1",
    interface = "org.worldcoin.OrbUserEvent1"
)]
trait OutboundInterface {
    fn user_event(&self, event: String) -> zbus::fdo::Result<()>;
}
