//! Encoding of the shared `pcp-defs` payload messages into package files.
//!
//! Messages are written as given. JSON is compact UTF-8 with object keys sorted
//! at every level and no trailing newline; absent optional fields are omitted.

use data_encoding::BASE64;
use orb_pcp_defs::{
    prost::Message,
    v1::{self, DiIrisEmbeddingShares, DiIrisEmbeddings, IrisCodeShares, IrisCodes},
};
use serde_json::Value;

pub struct BackendKey<'a> {
    /// Raw 32-byte public key; serialized as padded standard Base64.
    pub public_key: &'a [u8; 32],
    /// Already-encoded encrypted private key, preserved verbatim as a string.
    pub encrypted_private_key: &'a str,
}

pub struct BackendKeys<'a> {
    pub iris: BackendKey<'a>,
    pub normalized_iris: BackendKey<'a>,
    pub face: BackendKey<'a>,
    pub tier2: BackendKey<'a>,
}

/// Compact JSON with object keys sorted at every level. Sorting is explicit so it
/// does not depend on `serde_json`'s `preserve_order` feature.
pub(crate) fn sorted_json(value: Value) -> Result<Vec<u8>, serde_json::Error> {
    fn sort(value: Value) -> Value {
        match value {
            Value::Object(map) => {
                let mut entries: Vec<_> = map.into_iter().collect();
                entries.sort_by(|(a, _), (b, _)| a.cmp(b));
                Value::Object(
                    entries
                        .into_iter()
                        .map(|(key, value)| (key, sort(value)))
                        .collect(),
                )
            }
            Value::Array(values) => {
                Value::Array(values.into_iter().map(sort).collect())
            }
            value => value,
        }
    }
    serde_json::to_vec(&sort(value))
}

pub(crate) fn face_embeddings(
    embeddings: &[v1::FaceEmbedding],
) -> Result<Vec<u8>, serde_json::Error> {
    sorted_json(serde_json::to_value(embeddings)?)
}

pub(crate) struct EncodedDaugman {
    pub codes: Option<Vec<u8>>,
    pub shares: [Option<Vec<u8>>; 3],
}

pub(crate) fn encode_daugman(
    codes: Option<&IrisCodes>,
    shares: [Option<&IrisCodeShares>; 3],
) -> Result<EncodedDaugman, serde_json::Error> {
    let [first, second, third] = shares.map(|share| {
        share
            .map(|share| serde_json::to_value(share).and_then(sorted_json))
            .transpose()
    });
    Ok(EncodedDaugman {
        codes: codes
            .map(|codes| serde_json::to_value(codes).and_then(sorted_json))
            .transpose()?,
        shares: [first?, second?, third?],
    })
}

pub(crate) fn backend_keys(
    keys: &BackendKeys<'_>,
) -> Result<Vec<u8>, serde_json::Error> {
    let key = |key: &BackendKey<'_>| {
        Some(v1::BackendKey {
            public_key: Some(BASE64.encode(key.public_key)),
            encrypted_private_key: Some(key.encrypted_private_key.to_owned()),
        })
    };
    sorted_json(serde_json::to_value(v1::BackendKeys {
        iris: key(&keys.iris),
        normalized_iris: key(&keys.normalized_iris),
        face: key(&keys.face),
        tier2: key(&keys.tier2),
    })?)
}

pub(crate) struct EncodedDi {
    pub embeddings: Option<Vec<u8>>,
    pub shares: [Option<Vec<u8>>; 3],
}

/// Default protobuf messages encode to zero-byte files.
pub(crate) fn encode_di(
    embeddings: Option<&DiIrisEmbeddings>,
    shares: [Option<&DiIrisEmbeddingShares>; 3],
) -> EncodedDi {
    EncodedDi {
        embeddings: embeddings.map(Message::encode_to_vec),
        shares: shares.map(|share| share.map(Message::encode_to_vec)),
    }
}

#[cfg(test)]
#[path = "../tests/unit/payload.rs"]
mod tests;
