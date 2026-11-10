use orb_pcp_defs::{
    prost::Message,
    v1::{DiIrisEmbedding5PartyShareV1, DiIrisEmbedding5PartyShares, Hashes},
};
use serde_json::json;

#[test]
fn five_party_bundle_preserves_four_vectors_and_metadata() {
    let original = DiIrisEmbedding5PartyShares {
        share_v1: Some(DiIrisEmbedding5PartyShareV1 {
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
        DiIrisEmbedding5PartyShares::decode(wire.as_slice()).unwrap(),
        original
    );
    let json = serde_json::to_value(&original).unwrap();
    assert_eq!(
        serde_json::from_value::<DiIrisEmbedding5PartyShares>(json).unwrap(),
        original
    );
}

#[test]
fn five_party_hashes_use_filenames_and_preserve_existing_fields() {
    let expected = json!({
        "reference_image.pb": "00".repeat(32),
        "di_iris_embeddings_shares_0.pb": "11".repeat(32),
        "di_iris_embeddings_5_party_shares_0.pb": "22".repeat(32),
        "di_iris_embeddings_5_party_shares_1.pb": "33".repeat(32),
        "di_iris_embeddings_5_party_shares_2.pb": "44".repeat(32),
        "di_iris_embeddings_5_party_shares_3.pb": "55".repeat(32),
        "di_iris_embeddings_5_party_shares_4.pb": "66".repeat(32),
    });
    let hashes: Hashes = serde_json::from_value(expected.clone()).unwrap();
    let wire = hashes.encode_to_vec();
    let decoded = Hashes::decode(wire.as_slice()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), expected);
}

#[test]
fn legacy_manifest_omits_absent_five_party_hashes() {
    let expected = json!({"di_iris_embeddings_shares_0.pb": "11".repeat(32)});
    let hashes: Hashes = serde_json::from_value(expected.clone()).unwrap();
    let wire = hashes.encode_to_vec();
    let decoded = Hashes::decode(wire.as_slice()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), expected);
}

#[test]
fn five_party_share_payload_is_zeroized() {
    use zeroize::{Zeroize, ZeroizeOnDrop};

    fn assert_zeroize_on_drop<T: ZeroizeOnDrop>() {}
    assert_zeroize_on_drop::<DiIrisEmbedding5PartyShareV1>();
    let mut share = DiIrisEmbedding5PartyShareV1 {
        model_version: "test-model".into(),
        shares_version: "test-sharing-revision".into(),
        embedding_version: "test-embedding".into(),
        left_share: vec![1; 512],
        left_mirror_share: vec![2; 512],
        right_share: vec![3; 512],
        right_mirror_share: vec![4; 512],
    };
    share.zeroize();
    assert_eq!(share, DiIrisEmbedding5PartyShareV1::default());
}
