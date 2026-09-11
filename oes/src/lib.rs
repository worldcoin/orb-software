use serde::{Deserialize, Serialize};

pub mod connd;
pub mod core;
pub mod system;

pub use connd::*;
pub use core::*;
pub use system::*;
use zenorb::zenoh::bytes::OptionZBytes;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub enum Mode {
    /// Forwards the message immedately through the OES
    Normal,
    /// Forwards the message immedately through the OES but also stores it in the cache.
    /// The last cached message of each event is always sent to the backend every 30s.
    Sticky,
    /// Forwards the message strictly to the OES cache.
    /// The last cached message of each event is always sent to the backend every 30s.
    CacheOnly,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct Headers {
    pub mode: Mode,
    /// Occurrence time in Unix milliseconds, captured before buffering or retrying.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_occurrence_timestamp"
    )]
    pub created_at: Option<i64>,
}

fn deserialize_occurrence_timestamp<'de, D>(
    deserializer: D,
) -> Result<Option<i64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    // Optional timestamp metadata must not invalidate an otherwise valid mode.
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value.and_then(|value| value.as_i64()))
}

impl Default for Headers {
    fn default() -> Self {
        Self {
            mode: Mode::Normal,
            created_at: None,
        }
    }
}

impl Headers {
    pub fn mode(mut self, mode: Mode) -> Self {
        self.mode = mode;
        self
    }

    pub fn created_at(mut self, created_at: i64) -> Self {
        self.created_at = Some(created_at);
        self
    }
}

impl From<Headers> for OptionZBytes {
    fn from(val: Headers) -> Self {
        serde_json::to_vec(&val).ok().into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{from_value, json, to_value};

    #[test]
    fn occurrence_timestamp_is_optional_for_legacy_publishers() {
        for mode in [Mode::Normal, Mode::Sticky, Mode::CacheOnly] {
            let legacy = json!({"mode": mode});
            let headers: Headers = from_value(legacy.clone()).unwrap();
            assert_eq!(headers.mode, mode);
            assert_eq!(headers.created_at, None);
            assert_eq!(to_value(headers).unwrap(), legacy);

            let headers = Headers::default().mode(mode).created_at(1788653350123);
            let encoded = to_value(&headers).unwrap();
            assert_eq!(encoded["created_at"], 1788653350123_i64);
            assert_eq!(from_value::<Headers>(encoded).unwrap(), headers);

            for invalid in [
                json!(null),
                json!("invalid"),
                json!(false),
                json!({}),
                json!(1.5),
            ] {
                let headers: Headers =
                    from_value(json!({"mode": mode, "created_at": invalid})).unwrap();
                assert_eq!(headers.mode, mode);
                assert_eq!(headers.created_at, None);
            }
        }
    }
}
