use crate::collectors::{oes, ZenorbCtx};
use chrono::Utc;
use std::time::{Duration, Instant};
use tracing::{debug, warn};
use zenorb::Receiver;

/// Strips the orb-id prefix (everything before the first `/`) from a
/// zenoh key, returning the remainder.
///
/// Returns `None` if the key contains no `/` or the remainder is empty.
fn extract_event_name(key: &str) -> Option<&str> {
    let remainder = key.split_once('/')?.1;
    if remainder.is_empty() {
        return None;
    }

    Some(remainder)
}

/// Core reroute logic extracted for testability.
///
/// Checks throttle, and if the event should be forwarded, sends it
/// via `oes_tx`. Returns `true` if the event was forwarded.
fn try_reroute_event(
    ctx: &ZenorbCtx,
    event_name: &str,
    throttle: Duration,
    payload: Option<serde_json::Value>,
) -> bool {
    let mut throttle_map = ctx.oes_throttle.lock().unwrap();
    if let Some(last_sent) = throttle_map.get(event_name)
        && last_sent.elapsed() < throttle
    {
        debug!(
            event_name = %event_name,
            "Throttling OES reroute event"
        );

        return false;
    }
    throttle_map.insert(event_name.to_string(), Instant::now());
    drop(throttle_map);

    let event = oes::Event {
        name: event_name.to_string(),
        created_at: Utc::now(),
        payload,
    };

    if let Err(e) = ctx.oes_tx.send(event) {
        warn!("Failed to send rerouted OES event: {e}");
    }

    true
}

pub(crate) trait OesReroute {
    fn oes_reroute(
        self,
        keyexpr: impl Into<String>,
        query_timeout: Duration,
        throttle: Duration,
    ) -> Self;
}

impl<'a> OesReroute for Receiver<'a, ZenorbCtx> {
    fn oes_reroute(
        self,
        keyexpr: impl Into<String>,
        query_timeout: Duration,
        throttle: Duration,
    ) -> Self {
        self.querying_subscriber(
            keyexpr,
            query_timeout,
            move |ctx, sample| async move {
                let key = sample.key_expr().to_string();

                let name = match extract_event_name(&key) {
                    Some(name) => name.to_string(),
                    None => {
                        warn!(
                            "Failed to extract event name from key: \
                             {key}"
                        );

                        return Ok(());
                    }
                };

                let payload = match sample.payload().try_to_string() {
                    Ok(s) => oes::decode_payload(sample.encoding(), &s),
                    Err(_) => None,
                };

                try_reroute_event(&ctx, &name, throttle, payload);

                Ok(())
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        collectors::connectivity::GlobalConnectivity,
        dbus::intf_impl::BackendStatusImpl,
    };
    use proptest::prelude::*;
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };
    use tokio::sync::watch;

    // -- extract_event_name smoke tests --

    #[test]
    fn extract_simple_key() {
        assert_eq!(
            extract_event_name("bfd00a01/signup/capture_started"),
            Some("signup/capture_started"),
        );
    }

    #[test]
    fn extract_single_segment() {
        assert_eq!(extract_event_name("bfd00a01/status"), Some("status"),);
    }

    #[test]
    fn extract_multi_segment() {
        assert_eq!(
            extract_event_name("bfd00a01/deep/nested/path"),
            Some("deep/nested/path"),
        );
    }

    #[test]
    fn extract_no_slash_returns_none() {
        assert_eq!(extract_event_name("bfd00a01"), None);
    }

    #[test]
    fn extract_empty_returns_none() {
        assert_eq!(extract_event_name(""), None);
    }

    #[test]
    fn extract_trailing_slash_returns_none() {
        assert_eq!(extract_event_name("bfd00a01/"), None);
    }

    // -- proptest tests for extract_event_name --

    fn segments(
        count: std::ops::RangeInclusive<usize>,
    ) -> impl Strategy<Value = String> {
        prop::collection::vec("[a-z_]{1,16}", count).prop_map(|segs| segs.join("/"))
    }

    proptest! {
        #[test]
        fn prop_roundtrip(
            orb_id in "[a-f0-9]{1,16}",
            rest in segments(1..=4),
        ) {
            let key = format!("{orb_id}/{rest}");
            prop_assert_eq!(
                extract_event_name(&key),
                Some(rest.as_str()),
            );
        }

        #[test]
        fn prop_no_slash_returns_none(s in "[a-z0-9]{0,32}") {
            prop_assert_eq!(extract_event_name(&s), None);
        }

        #[test]
        fn prop_trailing_slash_returns_none(
            orb_id in "[a-f0-9]{1,16}",
        ) {
            let key = format!("{orb_id}/");
            prop_assert_eq!(extract_event_name(&key), None);
        }

        #[test]
        fn prop_multi_segment_preserved(
            orb_id in "[a-f0-9]{1,16}",
            seg1 in "[a-z_]{1,8}",
            seg2 in "[a-z_]{1,8}",
            seg3 in "[a-z_]{1,8}",
        ) {
            let rest = format!("{seg1}/{seg2}/{seg3}");
            let key = format!("{orb_id}/{rest}");
            prop_assert_eq!(
                extract_event_name(&key),
                Some(rest.as_str()),
            );
        }
    }

    // -- helper to construct a test ZenorbCtx --

    fn make_test_ctx() -> (ZenorbCtx, flume::Receiver<oes::Event>) {
        let (oes_tx, oes_rx) = flume::unbounded();
        let (connectivity_tx, _) = watch::channel(GlobalConnectivity::NotConnected);

        let ctx = ZenorbCtx {
            backend_status: BackendStatusImpl::new(),
            connectivity_tx,
            hardware_states: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            front_als: Arc::new(tokio::sync::Mutex::new(None)),
            oes_tx,
            oes_throttle: Arc::new(Mutex::new(HashMap::new())),
        };

        (ctx, oes_rx)
    }

    // -- throttle unit tests --

    #[test]
    fn first_event_is_forwarded() {
        let (ctx, oes_rx) = make_test_ctx();
        let throttle = Duration::from_millis(100);

        let forwarded =
            try_reroute_event(&ctx, "signup/capture_started", throttle, None);

        assert!(forwarded);
        let event = oes_rx.try_recv().unwrap();
        assert_eq!(event.name, "signup/capture_started");
        assert!(event.payload.is_none());
    }

    #[test]
    fn event_within_throttle_window_is_skipped() {
        let (ctx, oes_rx) = make_test_ctx();
        let throttle = Duration::from_secs(10);

        let first = try_reroute_event(&ctx, "signup/capture_started", throttle, None);
        assert!(first);
        assert!(oes_rx.try_recv().is_ok());

        let second = try_reroute_event(&ctx, "signup/capture_started", throttle, None);
        assert!(!second);
        assert!(oes_rx.try_recv().is_err());
    }

    #[test]
    fn event_after_throttle_window_is_forwarded() {
        let (ctx, oes_rx) = make_test_ctx();
        let throttle = Duration::from_millis(1);

        let first = try_reroute_event(&ctx, "signup/done", throttle, None);
        assert!(first);
        assert!(oes_rx.try_recv().is_ok());

        // Sleep past the throttle window
        std::thread::sleep(Duration::from_millis(5));

        let second = try_reroute_event(&ctx, "signup/done", throttle, None);
        assert!(second);
        assert!(oes_rx.try_recv().is_ok());
    }

    #[test]
    fn different_events_are_throttled_independently() {
        let (ctx, oes_rx) = make_test_ctx();
        let throttle = Duration::from_secs(10);

        assert!(try_reroute_event(
            &ctx,
            "signup/capture_started",
            throttle,
            None,
        ));
        assert!(oes_rx.try_recv().is_ok());

        // Same event is throttled
        assert!(!try_reroute_event(
            &ctx,
            "signup/capture_started",
            throttle,
            None,
        ));
        assert!(oes_rx.try_recv().is_err());

        // Different event is not throttled
        assert!(try_reroute_event(
            &ctx,
            "signup/capture_completed",
            throttle,
            None,
        ));
        let event = oes_rx.try_recv().unwrap();
        assert_eq!(event.name, "signup/capture_completed");
    }

    #[test]
    fn payload_is_forwarded_correctly() {
        let (ctx, oes_rx) = make_test_ctx();
        let throttle = Duration::from_millis(100);
        let payload = Some(serde_json::json!({"key": "value", "num": 42}));

        try_reroute_event(&ctx, "signup/data", throttle, payload.clone());

        let event = oes_rx.try_recv().unwrap();
        assert_eq!(event.name, "signup/data");
        assert_eq!(event.payload, payload);
    }
}
