use crate::collectors::{oes, ZenorbCtx};
use chrono::Utc;
use std::time::{Duration, Instant};
use tracing::{debug, warn};
use zenorb::Receiver;

/// Strips the orb-id prefix (everything before the first `/`) from a
/// zenoh key, returning the remainder.
///
/// Returns `None` if the key contains no `/` or the remainder is empty.
pub(crate) fn extract_event_name(key: &str) -> Option<&str> {
    let remainder = key.split_once('/')?.1;
    if remainder.is_empty() {
        return None;
    }

    Some(remainder)
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
                            "Failed to extract event name from key: {key}"
                        );

                        return Ok(());
                    }
                };

                // Check throttle
                {
                    let mut throttle_map =
                        ctx.oes_throttle.lock().unwrap();
                    if let Some(last_sent) = throttle_map.get(&name) {
                        if last_sent.elapsed() < throttle {
                            debug!(
                                event_name = %name,
                                "Throttling OES reroute event"
                            );

                            return Ok(());
                        }
                    }
                    throttle_map
                        .insert(name.clone(), Instant::now());
                }

                let payload = match sample.payload().try_to_string() {
                    Ok(s) => {
                        oes::decode_payload(sample.encoding(), &s)
                    }
                    Err(_) => None,
                };

                let event = oes::Event {
                    name,
                    created_at: Utc::now(),
                    payload,
                };

                if let Err(e) = ctx.oes_tx.send(event) {
                    warn!(
                        "Failed to send rerouted OES event: {e}"
                    );
                }

                Ok(())
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_event_name_simple() {
        assert_eq!(
            extract_event_name("bfd00a01/signup/capture_started"),
            Some("signup/capture_started"),
        );
    }

    #[test]
    fn test_extract_event_name_single_segment() {
        assert_eq!(
            extract_event_name("bfd00a01/status"),
            Some("status"),
        );
    }

    #[test]
    fn test_extract_event_name_multi_segment() {
        assert_eq!(
            extract_event_name("bfd00a01/deep/nested/path"),
            Some("deep/nested/path"),
        );
    }

    #[test]
    fn test_extract_event_name_no_slash() {
        assert_eq!(extract_event_name("bfd00a01"), None);
    }

    #[test]
    fn test_extract_event_name_empty() {
        assert_eq!(extract_event_name(""), None);
    }

    #[test]
    fn test_extract_event_name_trailing_slash() {
        assert_eq!(extract_event_name("bfd00a01/"), None);
    }
}
