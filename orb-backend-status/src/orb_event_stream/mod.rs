use crate::backend::client::StatusClient;
use chrono::Utc;
use color_eyre::{eyre::eyre, Result};
use eyre::ContextCompat;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::task;
use tokio_util::sync::CancellationToken;
use tracing::warn;
use zenorb::zenoh::{bytes::Encoding, sample::Sample};

pub mod reroute;

mod flusher;

pub const KEY_EXPR: &str = "**/oes/**";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub name: String,
    pub created_at: i64,
    pub payload: Option<serde_json::Value>,
}

pub struct Payload {
    pub headers: oes::Headers,
    pub event: Event,
}

#[derive(Debug)]
struct Throttle {
    value: Duration,
    last_publish: Option<Instant>,
}

#[derive(Debug, Clone)]
pub struct OrbEventStream {
    tx: flume::Sender<Event>,
    cache: Arc<Mutex<HashMap<String, Event>>>,
    throttle: Arc<Mutex<HashMap<String, Throttle>>>,
}

impl OrbEventStream {
    pub fn start(status_client: StatusClient, cancel_token: CancellationToken) -> Self {
        let (tx, rx) = flume::unbounded();

        task::spawn(flusher::run_oes_flush_loop(rx, status_client, cancel_token));

        Self {
            tx,
            cache: Default::default(),
            throttle: Default::default(),
        }
    }

    /// Returns a clone of all currently cached OES events
    pub fn cached(&self) -> Result<Vec<Event>> {
        let values = self
            .cache
            .lock()
            .map_err(|_| eyre!("cache lock poison"))?
            .values()
            .cloned()
            .collect();

        Ok(values)
    }

    pub fn throttle(&self, throttles: &[(&str, Duration)]) -> Result<()> {
        let mut throttle_map = self
            .throttle
            .lock()
            .map_err(|_| eyre!("throttle lock poison"))?;

        for (evt_name, throttle) in throttles {
            throttle_map.insert(
                evt_name.to_string(),
                Throttle {
                    value: *throttle,
                    last_publish: None,
                },
            );
        }

        Ok(())
    }

    pub fn ingest(&self, payload: Payload) -> Result<()> {
        match payload.headers.mode {
            oes::Mode::CacheOnly => {
                let mut cache =
                    self.cache.lock().map_err(|_| eyre!("cache lock poison"))?;
                cache.insert(payload.event.name.clone(), payload.event);
            }

            oes::Mode::Sticky => {
                let mut cache =
                    self.cache.lock().map_err(|_| eyre!("cache lock poison"))?;
                cache.insert(payload.event.name.clone(), payload.event.clone());

                if self.is_throttled(&payload.event.name)? {
                    return Ok(());
                }

                let _ = self.tx.send(payload.event).inspect_err(|e| {
                    warn!("Failed to send OES event over channel: {e}")
                });
            }

            oes::Mode::Normal => {
                if self.is_throttled(&payload.event.name)? {
                    return Ok(());
                }

                let _ = self.tx.send(payload.event).inspect_err(|e| {
                    warn!("Failed to send OES event over channel: {e}")
                });
            }
        }

        Ok(())
    }

    fn is_throttled(&self, evt_name: &str) -> Result<bool> {
        let can_send = match self
            .throttle
            .lock()
            .map_err(|_| eyre!("throttle lock poison"))?
            .get_mut(evt_name)
        {
            None => false,

            Some(throttle) => {
                if throttle
                    .last_publish
                    .is_some_and(|lp| lp.elapsed() < throttle.value)
                {
                    true
                } else {
                    throttle.last_publish = Some(Instant::now());
                    false
                }
            }
        };

        Ok(can_send)
    }
}

impl TryFrom<Sample> for Payload {
    type Error = color_eyre::Report;

    fn try_from(sample: Sample) -> Result<Self> {
        let headers: oes::Headers = sample
            .attachment()
            .and_then(|zbytes| {
                serde_json::from_slice(&zbytes.to_bytes())
                    .inspect_err(|e| warn!("failed to deserialize oes headers: {e:?}"))
                    .ok()
            })
            .unwrap_or_default();

        let key = sample.key_expr().to_string();

        let name = extract_event_name(&key).wrap_err_with(|| {
            format!("Failed to extract event name from OES key: {key}")
        })?;

        let payload = sample
            .payload()
            .try_to_string()
            .ok()
            .and_then(|s| match *sample.encoding() {
                Encoding::APPLICATION_JSON => serde_json::from_str(&s).ok(),
                Encoding::TEXT_PLAIN => Some(serde_json::Value::String(s.to_string())),
                _ => None,
            });

        let created_at = Utc::now().timestamp_millis();

        let event = Event {
            name,
            created_at,
            payload,
        };

        Ok(Payload { headers, event })
    }
}

/// Extracts the event name from a zenoh key expression.
///
/// The key has the form `<orb_id>/<namespace>/oes/<event_name>`.
/// We strip the orb_id prefix (first segment), then split on `/oes/`
/// to get the namespace and event_name, producing
/// `<namespace>/<event_name>`.
///
/// If there is no `/oes/` we simply remove the orb id segment.
///
/// Examples:
/// - `bfd00a01/signup/oes/capture_started` -> `signup/capture_started`
/// - `bfd00a01/deep/nested/ns/oes/my_event` -> `deep/nested/ns/my_event`
/// - `bfd00a01/ns/my_event` -> `ns/my_event`
fn extract_event_name(key: &str) -> Option<String> {
    // Strip the orb_id prefix (everything up to and including the
    // first `/`)
    let without_orb_id = key.split_once('/')?.1;

    // Split on `/oes/` to separate namespace from event_name
    // if no `/oes/` then we consider the event to be
    let Some((namespace, event_name)) = without_orb_id.split_once("/oes/") else {
        if without_orb_id.starts_with("oes/")
            || without_orb_id.is_empty()
            || without_orb_id.chars().all(|c| !c.is_alphanumeric())
        {
            return None;
        }

        return Some(without_orb_id.to_string());
    };

    if namespace.is_empty() || event_name.is_empty() {
        return None;
    }

    Some(format!("{namespace}/{event_name}"))
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

        assert_eq!(
            extract_event_name("bfd00a01/signup/capture"),
            Some("signup/capture".to_string())
        );

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
        fn prop_no_oes_marker_returns_rest(
            orb_id in "[a-f0-9]{1,16}",
            rest in "[a-z_/]{1,32}"
                .prop_filter("must not contain /oes/", |s| {
                    !s.contains("/oes/")
                })
                .prop_filter("must start with and alphabetic character", |s| {
                    s.chars().next().is_some_and(|x| x.is_alphabetic())
                }),
        ) {
            let key = format!("{orb_id}/{rest}");
            prop_assert_eq!(extract_event_name(&key), Some(rest));
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
}
