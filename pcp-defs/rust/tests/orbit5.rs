use orb_pcp_defs::{
    prost::Message,
    v1::{Hashes, Orbit5IrisEmbeddingShareV1, Orbit5IrisEmbeddingShares},
};
use serde_json::json;

#[test]
fn orbit5_bundle_preserves_four_vectors_and_metadata() {
    let original = Orbit5IrisEmbeddingShares {
        share_v1: Some(Orbit5IrisEmbeddingShareV1 {
            model_version: "test-model".into(),
            shares_version: "test-sharing-revision".into(),
            embedding_version: "test-embedding".into(),
            left_share: vec![0; 512],
            left_mirror_share: vec![1; 512],
            right_share: vec![32768; 512],
            right_mirror_share: vec![u32::from(u16::MAX); 512],
        }),
    };

    let wire = original.encode_to_vec();
    assert_eq!(
        Orbit5IrisEmbeddingShares::decode(wire.as_slice()).unwrap(),
        original
    );
    let json = serde_json::to_value(&original).unwrap();
    assert_eq!(
        serde_json::from_value::<Orbit5IrisEmbeddingShares>(json).unwrap(),
        original
    );
}

#[test]
fn orbit5_hashes_use_filenames_and_preserve_existing_fields() {
    let expected = json!({
        "reference_image.pb": "00".repeat(32),
        "di_iris_embeddings_shares_0.pb": "11".repeat(32),
        "orbit5_iris_embeddings_shares_0.pb": "22".repeat(32),
        "orbit5_iris_embeddings_shares_1.pb": "33".repeat(32),
        "orbit5_iris_embeddings_shares_2.pb": "44".repeat(32),
        "orbit5_iris_embeddings_shares_3.pb": "55".repeat(32),
        "orbit5_iris_embeddings_shares_4.pb": "66".repeat(32),
    });
    let hashes: Hashes = serde_json::from_value(expected.clone()).unwrap();
    let wire = hashes.encode_to_vec();
    let decoded = Hashes::decode(wire.as_slice()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), expected);
}

#[test]
fn legacy_manifest_omits_absent_orbit5_hashes() {
    let expected = json!({"di_iris_embeddings_shares_0.pb": "11".repeat(32)});
    let hashes: Hashes = serde_json::from_value(expected.clone()).unwrap();
    let wire = hashes.encode_to_vec();
    let decoded = Hashes::decode(wire.as_slice()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), expected);
}
