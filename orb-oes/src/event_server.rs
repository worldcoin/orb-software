use crate::stream::{Event, OrbEventStream, Payload};
use chrono::Utc;
use orb_oes_binder::{
    BnOesEventStream, OesEventStream as OesEventStreamIface, OesEventStreamT,
};
use tracing::warn;

/// Well-known name `IOesEventStream` is registered under with Android's
/// service manager.
const SERVICE_NAME: &str = "org.worldcoin.OesEventStream";

struct Handler {
    oes: OrbEventStream,
}

impl OesEventStreamT for Handler {
    fn push_event(&self, name: String, payload_json: String, mode: i32) {
        let event = Event {
            name,
            created_at: Utc::now().timestamp_millis(),
            payload: serde_json::from_str(&payload_json).ok(),
        };

        let payload = Payload {
            headers: oes::Headers::default().mode(mode_from_i32(mode)),
            event,
        };

        if let Err(e) = self.oes.ingest(payload) {
            warn!("failed to ingest OES event pushed over binder: {e:?}");
        }
    }
}

fn mode_from_i32(mode: i32) -> oes::Mode {
    match mode {
        1 => oes::Mode::Sticky,
        2 => oes::Mode::CacheOnly,
        _ => oes::Mode::Normal,
    }
}

/// Register the `IOesEventStream` binder service. Requires
/// `rsbinder::ProcessState::init_default`/`start_thread_pool` to have
/// already been called.
///
/// # Errors
/// - if the service can't be registered with the service manager
pub fn register(oes: OrbEventStream) -> eyre::Result<()> {
    let service = OesEventStreamIface(Handler { oes });
    let binder = BnOesEventStream::new_binder(service);

    rsbinder::hub::add_service(SERVICE_NAME, binder.as_binder()).map_err(|e| {
        eyre::eyre!("failed to register {SERVICE_NAME} binder service: {e}")
    })?;

    Ok(())
}
