use crate::collectors::oes::Event;
use eyre::Result;
use tracing::info;

pub struct OesClient {
    pub use_placeholder: bool,
}

impl OesClient {
    pub async fn send_events(&self, events: &[Event]) -> Result<()> {
        if self.use_placeholder {
            info!(count = events.len(), "OES flush (placeholder)");
            for event in events {
                info!(
                    name = %event.name,
                    created_at = event.created_at,
                    payload = ?event.payload,
                    "OES event",
                );
            }

            return Ok(());
        }

        // TODO: POST events to the real OES backend endpoint
        unimplemented!(
            "Real OES backend endpoint not yet implemented"
        );
    }
}
