use super::ZenorbCtx;
use chrono::{DateTime, Utc};
use color_eyre::Result;
use serde::{Deserialize, Serialize};
use tracing::warn;
use zenorb::zenoh::{self, bytes::Encoding};

pub const OES_KEY_EXPR: &str = "**/oes/**";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub name: String,
    pub created_at: DateTime<Utc>,
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

    let payload = match sample.payload().try_to_string() {
        Ok(s) => decode_payload(sample.encoding(), &s),
        Err(_) => None,
    };

    let created_at = Utc::now();

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

pub(crate) fn decode_payload(encoding: &Encoding, payload_str: &str) -> Option<serde_json::Value> {
    if *encoding == Encoding::APPLICATION_JSON {
        serde_json::from_str(payload_str).ok()
    } else if *encoding == Encoding::TEXT_PLAIN {
        Some(serde_json::Value::String(payload_str.to_string()))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn test_extract_event_name_smoke() {
        assert_eq!(
            extract_event_name("bfd00a01/signup/oes/capture_started"),
            Some("signup/capture_started".to_string()),
        );
        assert_eq!(
            extract_event_name("bfd00a01/deep/ns/oes/foo/bar"),
            Some("deep/ns/foo/bar".to_string()),
        );
        assert_eq!(extract_event_name("bfd00a01/signup/capture"), None);
        assert_eq!(extract_event_name(""), None);
    }

    fn segments(
        count: std::ops::RangeInclusive<usize>,
    ) -> impl Strategy<Value = String> {
        prop::collection::vec("[a-z_]{1,16}", count).prop_map(|segs| segs.join("/"))
    }

    proptest! {
        #[test]
        fn prop_roundtrip(
            orb_id in "[a-f0-9]{1,16}",
            ns in segments(1..=3),
            event in segments(1..=3),
        ) {
            let key = format!("{orb_id}/{ns}/oes/{event}");
            prop_assert_eq!(
                extract_event_name(&key),
                Some(format!("{ns}/{event}")),
            );
        }

        #[test]
        fn prop_no_oes_marker_returns_none(
            orb_id in "[a-f0-9]{1,16}",
            rest in "[a-z_/]{1,32}"
                .prop_filter("must not contain /oes/", |s| {
                    !s.contains("/oes/")
                }),
        ) {
            let key = format!("{orb_id}/{rest}");
            prop_assert_eq!(extract_event_name(&key), None);
        }

        #[test]
        fn prop_no_slash_returns_none(s in "[a-z0-9]{0,32}") {
            prop_assert_eq!(extract_event_name(&s), None);
        }

        #[test]
        fn prop_empty_namespace_returns_none(
            orb_id in "[a-f0-9]{1,16}",
            event in segments(1..=3),
        ) {
            let key = format!("{orb_id}/oes/{event}");
            prop_assert_eq!(extract_event_name(&key), None);
        }

        #[test]
        fn prop_empty_event_returns_none(
            orb_id in "[a-f0-9]{1,16}",
            ns in segments(1..=3),
        ) {
            let key = format!("{orb_id}/{ns}/oes/");
            prop_assert_eq!(extract_event_name(&key), None);
        }
    }

    #[test]
    fn test_decode_payload_json() {
        let payload = r#"{"key": "value", "num": 42}"#;
        let result = decode_payload(&Encoding::APPLICATION_JSON, payload);

        let expected: serde_json::Value =
            serde_json::json!({"key": "value", "num": 42});
        assert_eq!(result, Some(expected));
    }

    #[test]
    fn test_decode_payload_json_invalid() {
        let payload = "not valid json {";
        let result = decode_payload(&Encoding::APPLICATION_JSON, payload);
        assert_eq!(result, None);
    }

    #[test]
    fn test_decode_payload_text_plain() {
        let payload = "hello world";
        let result = decode_payload(&Encoding::TEXT_PLAIN, payload);
        assert_eq!(
            result,
            Some(serde_json::Value::String("hello world".to_string())),
        );
    }

    #[test]
    fn test_decode_payload_unsupported_encoding() {
        let payload = "some bytes";
        let result = decode_payload(&Encoding::APPLICATION_OCTET_STREAM, payload);
        assert_eq!(result, None);
    }
}
