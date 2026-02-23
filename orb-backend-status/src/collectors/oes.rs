use super::ZenorbCtx;
use color_eyre::Result;
use serde::Serialize;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::warn;
use zenorb::zenoh::{self, bytes::Encoding};

pub const OES_KEY_EXPR: &str = "**/oes/**";

#[derive(Debug, Clone, Serialize)]
pub struct Event {
    pub name: String,
    pub created_at: u64,
    pub payload: Option<serde_json::Value>,
}

/// Extracts the event name from a zenoh key expression.
///
/// The key has the form `<orb_id>/<namespace>/oes/<event_name>`.
/// We strip the orb_id prefix (first segment), then split on `/oes/`
/// to get the namespace and event_name, producing
/// `<namespace>/<event_name>`.
///
/// Examples:
/// - `bfd00a01/signup/oes/capture_started` -> `signup/capture_started`
/// - `bfd00a01/deep/nested/ns/oes/my_event` ->
///   `deep/nested/ns/my_event`
fn extract_event_name(key: &str) -> Option<String> {
    // Strip the orb_id prefix (everything up to and including the
    // first `/`)
    let without_orb_id = key.split_once('/')?.1;

    // Split on `/oes/` to separate namespace from event_name
    let (namespace, event_name) = without_orb_id.split_once("/oes/")?;

    if namespace.is_empty() || event_name.is_empty() {
        return None;
    }

    Some(format!("{namespace}/{event_name}"))
}

pub(crate) async fn handle_oes_event(
    ctx: ZenorbCtx,
    sample: zenoh::sample::Sample,
) -> Result<()> {
    let key = sample.key_expr().to_string();

    let name = match extract_event_name(&key) {
        Some(name) => name,
        None => {
            warn!("Failed to extract event name from OES key: {key}");

            return Ok(());
        }
    };

    let payload = decode_payload(&sample);

    let created_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let event = Event {
        name,
        created_at,
        payload,
    };

    if let Err(e) = ctx.oes_tx.send(event) {
        warn!("Failed to send OES event over channel: {e}");
    }

    Ok(())
}

fn decode_payload(sample: &zenoh::sample::Sample) -> Option<serde_json::Value> {
    let encoding = sample.encoding();

    if *encoding == Encoding::APPLICATION_JSON {
        let s = sample.payload().try_to_string().ok()?;
        serde_json::from_str(s.as_ref()).ok()
    } else if *encoding == Encoding::TEXT_PLAIN {
        let s = sample.payload().try_to_string().ok()?;
        Some(serde_json::Value::String(s.into_owned()))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_event_name_simple() {
        assert_eq!(
            extract_event_name("bfd00a01/signup/oes/capture_started"),
            Some("signup/capture_started".to_string()),
        );
    }

    #[test]
    fn test_extract_event_name_nested_namespace() {
        assert_eq!(
            extract_event_name("bfd00a01/deep/nested/ns/oes/my_event"),
            Some("deep/nested/ns/my_event".to_string()),
        );
    }

    #[test]
    fn test_extract_event_name_no_oes_marker() {
        assert_eq!(extract_event_name("bfd00a01/signup/capture_started"), None,);
    }

    #[test]
    fn test_extract_event_name_no_orb_id() {
        assert_eq!(extract_event_name("signup/oes/capture_started"), None,);
    }

    #[test]
    fn test_extract_event_name_empty_namespace() {
        assert_eq!(extract_event_name("bfd00a01/oes/capture_started"), None,);
    }

    #[test]
    fn test_extract_event_name_empty_event() {
        assert_eq!(extract_event_name("bfd00a01/signup/oes/"), None,);
    }

    #[test]
    fn test_extract_event_name_empty_string() {
        assert_eq!(extract_event_name(""), None,);
    }
}
