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
use zeroize::{Zeroize, ZeroizeOnDrop};

#[derive(Zeroize, ZeroizeOnDrop)]
pub struct BackendKey {
    /// Raw 32-byte public key; serialized as padded standard Base64.
    pub public_key: [u8; 32],
    /// Already-encoded encrypted private key, preserved verbatim as a string.
    pub encrypted_private_key: String,
}

#[derive(bon::Builder)]
#[builder(start_fn = face)]
#[builder(finish_fn = tier2)]
pub struct BackendKeys {
    #[builder(start_fn)]
    pub face: BackendKey,
    #[builder(finish_fn)]
    pub tier2: BackendKey,
    pub iris: BackendKey,
    pub normalized_iris: BackendKey,
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

#[derive(Zeroize, ZeroizeOnDrop)]
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

pub(crate) fn backend_keys(keys: &BackendKeys) -> Result<Vec<u8>, serde_json::Error> {
    let key = |key: &BackendKey| {
        Some(v1::BackendKey {
            public_key: Some(BASE64.encode(&key.public_key)),
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

#[derive(Zeroize, ZeroizeOnDrop)]
pub(crate) struct EncodedDi {
    pub embeddings: Vec<u8>,
    pub three_party_shares: [Vec<u8>; 3],
    pub five_party_shares: Option<[Vec<u8>; 5]>,
}

/// Default protobuf messages encode to zero-byte files.
pub(crate) fn encode_di(
    embeddings: &DiIrisEmbeddings,
    three_party_shares: [&DiIrisEmbeddingShares; 3],
    five_party_shares: Option<&[DiIrisEmbeddingShares; 5]>,
) -> EncodedDi {
    EncodedDi {
        embeddings: Message::encode_to_vec(embeddings),
        three_party_shares: three_party_shares.map(Message::encode_to_vec),
        five_party_shares: five_party_shares
            .map(|shares| shares.each_ref().map(Message::encode_to_vec)),
    }
}

pub(crate) const FIVE_PARTY_SHARE_FILES: [&str; 5] = [
    "di_iris_embeddings_5_party_shares_0.pb",
    "di_iris_embeddings_5_party_shares_1.pb",
    "di_iris_embeddings_5_party_shares_2.pb",
    "di_iris_embeddings_5_party_shares_3.pb",
    "di_iris_embeddings_5_party_shares_4.pb",
];

#[cfg(test)]
#[path = "../tests/unit/payload.rs"]
mod tests;
