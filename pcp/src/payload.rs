//! Encoding of the shared `pcp-defs` payload messages into package files.
//!
//! JSON output is compact UTF-8 with lexicographically sorted object keys and no
//! trailing newline, matching orb-core; absent optional fields are omitted.
//!
//! Messages are written as given: nothing here decodes or validates biometric
//! data, shares or encrypted keys, generates shares or verifies their
//! relationship to codes and embeddings. Redaction must bypass biometric
//! encoding in the higher-level builder. Returned buffers are not automatically
//! zeroized.

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

/// Compact JSON with object keys sorted at every level, as orb-core writes it.
/// Sorting is explicit, so it holds even with `serde_json`'s `preserve_order`.
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

/// Encodes an ordered list with sorted object keys. An absent list is `&[]`.
pub(crate) fn face_embeddings(
    embeddings: &[v1::FaceEmbedding],
) -> Result<Vec<u8>, serde_json::Error> {
    sorted_json(serde_json::to_value(embeddings)?)
}

pub(crate) struct EncodedDaugman {
    pub codes: Vec<u8>,
    pub shares: [Vec<u8>; 3],
}

/// Encodes `iris_codes.json` and the three same-index recipient share files.
pub(crate) fn encode_daugman(
    codes: &IrisCodes,
    shares: &[IrisCodeShares; 3],
) -> Result<EncodedDaugman, serde_json::Error> {
    let [first, second, third] = shares
        .each_ref()
        .map(|share| serde_json::to_value(share).and_then(sorted_json));
    Ok(EncodedDaugman {
        codes: sorted_json(serde_json::to_value(codes)?)?,
        shares: [first?, second?, third?],
    })
}

/// Encodes all four roles with sorted keys at both object levels.
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
    pub embeddings: Vec<u8>,
    pub shares: [Vec<u8>; 3],
}

/// Encodes the embedding file and three same-index recipient share files as
/// binary protobuf. Default messages, as orb-core writes without DI data,
/// encode to empty files.
pub(crate) fn encode_di(
    embeddings: &DiIrisEmbeddings,
    shares: &[DiIrisEmbeddingShares; 3],
) -> EncodedDi {
    EncodedDi {
        embeddings: embeddings.encode_to_vec(),
        shares: shares.each_ref().map(Message::encode_to_vec),
    }
}

#[cfg(test)]
#[path = "../tests/unit/payload.rs"]
mod tests;
