use orb_pcp_defs::prost::Message;
use orb_pcp_defs::v1::{
    DiIrisEmbeddingShareV1, DiIrisEmbeddingShares, DiIrisEmbeddingV1, DiIrisEmbeddings,
};

const VECTOR_LEN: usize = 512;

#[test]
fn embeddings_round_trip() {
    let i32_payload: Vec<i32> =
        (0..VECTOR_LEN).map(|i| ((i as i32) % 256) - 128).collect();
    let f32_payload: Vec<f32> = (0..VECTOR_LEN).map(|i| (i as f32) * 0.001).collect();

    let original = DiIrisEmbeddings {
        embedding_v1: Some(DiIrisEmbeddingV1 {
            model_version: "deep-identifier-1.0.0".into(),
            embedding_inference_backend: "deep-identifier".into(),
            embedding_version: "1.0.0".into(),
            left_embedding: i32_payload.clone(),
            left_mirror_embedding: i32_payload.clone(),
            right_embedding: i32_payload.clone(),
            right_mirror_embedding: i32_payload.clone(),
            left_embedding_f32: f32_payload.clone(),
            left_mirror_embedding_f32: f32_payload.clone(),
            right_embedding_f32: f32_payload.clone(),
            right_mirror_embedding_f32: f32_payload.clone(),
        }),
    };

    let wire = original.encode_to_vec();
    let decoded = DiIrisEmbeddings::decode(wire.as_slice())
        .expect("encode + decode should round-trip");

    assert_eq!(original, decoded);
}

#[test]
fn shares_round_trip() {
    let u32_payload: Vec<u32> = (0..VECTOR_LEN)
        .map(|i| ((i * 127) % 65536) as u32)
        .collect();

    let original = DiIrisEmbeddingShares {
        share_v1: Some(DiIrisEmbeddingShareV1 {
            model_version: "deep-identifier-1.0.0".into(),
            shares_version: "c2d631d821fe96827e8a92fd3bfd457afdd02b9e".into(),
            left_share: u32_payload.clone(),
            left_mirror_share: u32_payload.clone(),
            right_share: u32_payload.clone(),
            right_mirror_share: u32_payload.clone(),
            embedding_version: "1.0.0".into(),
        }),
    };

    let wire = original.encode_to_vec();
    let decoded = DiIrisEmbeddingShares::decode(wire.as_slice())
        .expect("encode + decode should round-trip");

    assert_eq!(original, decoded);
}

#[test]
fn embeddings_unset_round_trips_as_none() {
    let original = DiIrisEmbeddings { embedding_v1: None };

    let wire = original.encode_to_vec();
    let decoded = DiIrisEmbeddings::decode(wire.as_slice()).expect("decode");

    assert!(decoded.embedding_v1.is_none());
}
