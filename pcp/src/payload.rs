//! JSON and DI protobuf encoding from portable, already-prepared values.
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
use orb_pcp_defs::{
    prost::Message,
    v1::{
        DiIrisEmbeddingShareV1, DiIrisEmbeddingShares, DiIrisEmbeddingV1,
        DiIrisEmbeddings,
    },
};

pub struct FaceEmbedding<'a> {
    pub embedding: &'a str,
    pub embedding_type: &'a str,
    pub embedding_version: &'a str,
    pub embedding_inference_backend: &'a str,
}

/// Prepared iris codes and shares with common pipeline and sharing versions.
pub struct DaugmanData<'a> {
    /// Pipeline/library version, not a per-eye code version. Missing means JSON null.
    pub iris_version: Option<&'a str>,
    pub shares_version: &'a str,
    pub left: DaugmanEyeData<'a>,
    pub right: DaugmanEyeData<'a>,
}

/// Missing codes become JSON null; all three recipients' shares remain required.
/// Same-index shares belong to the same recipient across eyes and code/mask.
/// Encoding does not verify that shares reconstruct the supplied values.
pub struct DaugmanEyeData<'a> {
    pub iris_code: Option<&'a str>,
    pub mask_code: Option<&'a str>,
    pub iris_code_shares: [&'a str; 3],
    pub mask_code_shares: [&'a str; 3],
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
pub(crate) fn face_embeddings(
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

pub(crate) struct EncodedDaugman {
    pub codes: Vec<u8>,
    pub shares: [Vec<u8>; 3],
}

/// Encodes both eyes' codes/masks and three same-index recipient share files.
pub(crate) fn encode_daugman(
    data: &DaugmanData<'_>,
) -> Result<EncodedDaugman, serde_json::Error> {
    let codes = serde_json::to_vec(&BTreeMap::from([
        ("IRIS_version", data.iris_version),
        ("left_iris_code", data.left.iris_code),
        ("left_mask_code", data.left.mask_code),
        ("right_iris_code", data.right.iris_code),
        ("right_mask_code", data.right.mask_code),
    ]))?;
    let [first, second, third] = [0, 1, 2].map(|i| {
        serde_json::to_vec(&BTreeMap::from([
            ("IRIS_version", data.iris_version),
            ("IRIS_shares_version", Some(data.shares_version)),
            ("left_iris_code_shares", Some(data.left.iris_code_shares[i])),
            ("left_mask_code_shares", Some(data.left.mask_code_shares[i])),
            (
                "right_iris_code_shares",
                Some(data.right.iris_code_shares[i]),
            ),
            (
                "right_mask_code_shares",
                Some(data.right.mask_code_shares[i]),
            ),
        ]))
    });
    Ok(EncodedDaugman {
        codes,
        shares: [first?, second?, third?],
    })
}

/// Encodes all four roles with sorted keys at both object levels.
pub(crate) fn backend_keys(
    keys: &BackendKeys<'_>,
) -> Result<Vec<u8>, serde_json::Error> {
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

/// Prepared DI data with shared metadata. Before combining source eyes, the
/// consumer must check agreement on model version, embedding version and backend.
/// Encoding cannot recover or validate discarded per-eye source metadata.
pub struct DiData<'a> {
    pub model_version: &'a str,
    pub inference_backend: &'a str,
    pub embedding_version: &'a str,
    pub shares_version: &'a str,
    /// If either eye is absent, all four DI files are empty in the current format.
    pub left: Option<DiEyeData<'a>>,
    pub right: Option<DiEyeData<'a>>,
}

/// Already-quantized embeddings and shares for one eye. Encoding does not check
/// dimensions, floating-point validity or share reconstruction. Same-index
/// shares belong to the same recipient across eyes and original/mirrored values.
pub struct DiEyeData<'a> {
    pub embedding: &'a [i8],
    pub mirror_embedding: &'a [i8],
    pub embedding_f32: &'a [f32],
    pub mirror_embedding_f32: &'a [f32],
    pub embedding_shares: [&'a [u16]; 3],
    pub mirror_embedding_shares: [&'a [u16]; 3],
}

pub(crate) struct EncodedDi {
    pub embeddings: Vec<u8>,
    pub shares: [Vec<u8>; 3],
}

/// Encodes the embedding file and three same-index recipient share files.
///
/// For compatibility, if either eye is absent, all four buffers are empty and
/// the remaining eye is ignored. The caller must still include those empty files
/// in a non-redacted package. Both eyes present with empty vectors instead produce
/// present protobuf records. The caller supplies its sharing algorithm version.
pub(crate) fn encode_di(data: Option<&DiData<'_>>) -> EncodedDi {
    let Some(
        data @ DiData {
            left: Some(left),
            right: Some(right),
            ..
        },
    ) = data
    else {
        return EncodedDi {
            embeddings: Vec::new(),
            shares: std::array::from_fn(|_| Vec::new()),
        };
    };

    let embeddings = DiIrisEmbeddings {
        embedding_v1: Some(DiIrisEmbeddingV1 {
            model_version: data.model_version.to_owned(),
            embedding_inference_backend: data.inference_backend.to_owned(),
            embedding_version: data.embedding_version.to_owned(),
            left_embedding: left.embedding.iter().copied().map(i32::from).collect(),
            left_mirror_embedding: left
                .mirror_embedding
                .iter()
                .copied()
                .map(i32::from)
                .collect(),
            right_embedding: right.embedding.iter().copied().map(i32::from).collect(),
            right_mirror_embedding: right
                .mirror_embedding
                .iter()
                .copied()
                .map(i32::from)
                .collect(),
            left_embedding_f32: left.embedding_f32.to_vec(),
            left_mirror_embedding_f32: left.mirror_embedding_f32.to_vec(),
            right_embedding_f32: right.embedding_f32.to_vec(),
            right_mirror_embedding_f32: right.mirror_embedding_f32.to_vec(),
        }),
    }
    .encode_to_vec();

    let shares = std::array::from_fn(|i| {
        DiIrisEmbeddingShares {
            share_v1: Some(DiIrisEmbeddingShareV1 {
                model_version: data.model_version.to_owned(),
                shares_version: data.shares_version.to_owned(),
                embedding_version: data.embedding_version.to_owned(),
                left_share: left.embedding_shares[i]
                    .iter()
                    .copied()
                    .map(u32::from)
                    .collect(),
                left_mirror_share: left.mirror_embedding_shares[i]
                    .iter()
                    .copied()
                    .map(u32::from)
                    .collect(),
                right_share: right.embedding_shares[i]
                    .iter()
                    .copied()
                    .map(u32::from)
                    .collect(),
                right_mirror_share: right.mirror_embedding_shares[i]
                    .iter()
                    .copied()
                    .map(u32::from)
                    .collect(),
            }),
        }
        .encode_to_vec()
    });
    EncodedDi { embeddings, shares }
}

#[cfg(test)]
#[path = "../tests/unit/payload.rs"]
mod tests;
