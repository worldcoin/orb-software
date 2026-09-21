//! JSON payload encoding from portable, already-prepared values.
//!
//! Output is compact UTF-8 JSON with lexicographically sorted object keys and
//! no trailing newline. These payload formats are shared by PCP 2.7, 2.8 and 3.0.
//!
//! These encoders preserve strings without decoding or validating biometric
//! data, shares, or encrypted keys. They neither generate shares nor verify
//! their relationship to iris codes. Redaction must bypass biometric encoding
//! in the higher-level builder. Returned buffers are not automatically zeroized.

use std::collections::BTreeMap;

use data_encoding::BASE64;

pub struct FaceEmbedding<'a> {
    pub embedding: &'a str,
    pub embedding_type: &'a str,
    pub embedding_version: &'a str,
    pub embedding_inference_backend: &'a str,
}

/// Missing values are encoded as JSON `null`, not omitted.
pub struct IrisCodes<'a> {
    pub iris_version: Option<&'a str>,
    pub left_iris_code: Option<&'a str>,
    pub left_mask_code: Option<&'a str>,
    pub right_iris_code: Option<&'a str>,
    pub right_mask_code: Option<&'a str>,
}

/// One recipient's shares. All four share strings must be supplied.
///
/// The higher-level builder must supply the complete set of three recipients;
/// this encoder only serializes one recipient's record.
pub struct IrisCodeShare<'a> {
    pub iris_version: Option<&'a str>,
    pub iris_shares_version: &'a str,
    pub left_iris_code_shares: &'a str,
    pub left_mask_code_shares: &'a str,
    pub right_iris_code_shares: &'a str,
    pub right_mask_code_shares: &'a str,
}

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

/// Encodes an ordered list with sorted object keys. An absent list is `&[]`.
pub fn face_embeddings(
    embeddings: &[FaceEmbedding<'_>],
) -> Result<Vec<u8>, serde_json::Error> {
    let records: Vec<_> = embeddings
        .iter()
        .map(|embedding| {
            BTreeMap::from([
                ("embedding", embedding.embedding),
                ("embedding_type", embedding.embedding_type),
                ("embedding_version", embedding.embedding_version),
                (
                    "embedding_inference_backend",
                    embedding.embedding_inference_backend,
                ),
            ])
        })
        .collect();
    serde_json::to_vec(&records)
}

pub fn iris_codes(codes: &IrisCodes<'_>) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec(&BTreeMap::from([
        ("IRIS_version", codes.iris_version),
        ("left_iris_code", codes.left_iris_code),
        ("left_mask_code", codes.left_mask_code),
        ("right_iris_code", codes.right_iris_code),
        ("right_mask_code", codes.right_mask_code),
    ]))
}

pub fn iris_code_share(
    share: &IrisCodeShare<'_>,
) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec(&BTreeMap::from([
        ("IRIS_version", share.iris_version),
        ("IRIS_shares_version", Some(share.iris_shares_version)),
        ("left_iris_code_shares", Some(share.left_iris_code_shares)),
        ("left_mask_code_shares", Some(share.left_mask_code_shares)),
        ("right_iris_code_shares", Some(share.right_iris_code_shares)),
        ("right_mask_code_shares", Some(share.right_mask_code_shares)),
    ]))
}

/// Encodes all four roles with sorted keys at both object levels.
pub fn backend_keys(keys: &BackendKeys<'_>) -> Result<Vec<u8>, serde_json::Error> {
    let records: BTreeMap<_, _> = [
        ("iris", &keys.iris),
        ("normalized_iris", &keys.normalized_iris),
        ("face", &keys.face),
        ("tier2", &keys.tier2),
    ]
    .into_iter()
    .map(|(name, key)| {
        (
            name,
            BTreeMap::from([
                ("public_key", BASE64.encode(key.public_key)),
                (
                    "encrypted_private_key",
                    key.encrypted_private_key.to_owned(),
                ),
            ]),
        )
    })
    .collect();
    serde_json::to_vec(&records)
}
