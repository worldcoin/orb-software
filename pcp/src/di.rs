//! DI embedding and share protobuf encoding using the public PCP schemas.
//!
//! Inputs are already quantized and shared. This module checks paired metadata,
//! not vector dimensions, floating-point validity, or share reconstruction. It
//! preserves supplied numbers without normalization. Redaction belongs to the
//! higher-level builder; returned biometric buffers are not automatically zeroized.

use orb_pcp_defs::{
    prost::Message,
    v1::{
        DiIrisEmbeddingShareV1, DiIrisEmbeddingShares, DiIrisEmbeddingV1,
        DiIrisEmbeddings,
    },
};

pub struct Eye<'a> {
    pub model_version: &'a str,
    pub inference_backend: &'a str,
    pub embedding_version: &'a str,
    pub embedding: &'a [i8],
    pub mirror_embedding: &'a [i8],
    pub embedding_f32: &'a [f32],
    pub mirror_embedding_f32: &'a [f32],
    pub embedding_shares: [&'a [u16]; 3],
    pub mirror_embedding_shares: [&'a [u16]; 3],
}

pub struct Encoded {
    pub embeddings: Vec<u8>,
    pub shares: [Vec<u8>; 3],
}

/// Errors identify the mismatched field without including input values.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("DI model version differs between eyes")]
    ModelVersionMismatch,
    #[error("DI inference backend differs between eyes")]
    InferenceBackendMismatch,
    #[error("DI embedding version differs between eyes")]
    EmbeddingVersionMismatch,
}

/// Encodes the embedding file and three same-index recipient share files.
///
/// For compatibility, if either eye is absent, all four buffers are empty and
/// the remaining eye is ignored. The caller must still include those empty files
/// in a non-redacted package. Both eyes present with empty vectors instead produce
/// present protobuf records. The caller supplies its sharing algorithm version.
pub fn encode(
    left: Option<&Eye<'_>>,
    right: Option<&Eye<'_>>,
    shares_version: &str,
) -> Result<Encoded, Error> {
    let (Some(left), Some(right)) = (left, right) else {
        return Ok(Encoded {
            embeddings: Vec::new(),
            shares: std::array::from_fn(|_| Vec::new()),
        });
    };
    if left.model_version != right.model_version {
        return Err(Error::ModelVersionMismatch);
    }
    if left.inference_backend != right.inference_backend {
        return Err(Error::InferenceBackendMismatch);
    }
    if left.embedding_version != right.embedding_version {
        return Err(Error::EmbeddingVersionMismatch);
    }

    let embeddings = DiIrisEmbeddings {
        embedding_v1: Some(DiIrisEmbeddingV1 {
            model_version: left.model_version.to_owned(),
            embedding_inference_backend: left.inference_backend.to_owned(),
            embedding_version: left.embedding_version.to_owned(),
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
                model_version: left.model_version.to_owned(),
                shares_version: shares_version.to_owned(),
                embedding_version: left.embedding_version.to_owned(),
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
    Ok(Encoded { embeddings, shares })
}
