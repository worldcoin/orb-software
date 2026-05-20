//! Helper code to serialize and deserialize as base64 with standard alphabet.

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use serde::{Deserialize, Deserializer, Serializer};

pub(crate) fn serialize<S>(value: &[u8], serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let encoded = STANDARD.encode(value);
    serializer.serialize_str(&encoded)
}

pub(crate) fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
where
    D: Deserializer<'de>,
{
    let encoded = String::deserialize(deserializer)?;
    STANDARD
        .decode(encoded.as_bytes())
        .map_err(serde::de::Error::custom)
}
