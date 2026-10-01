use orb_pcp_defs::v1::{
    BackendKeys, FaceEmbedding, Hashes, Info, IrisCodeShares, IrisCodes,
};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/v2_8");

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!("{FIXTURES}/{name}"))
        .unwrap_or_else(|e| panic!("reading fixture {name}: {e}"))
}

#[test]
fn hashes_decodes_every_key_the_orb_writes() {
    let hashes: Hashes = serde_json::from_str(&fixture("hashes.json")).unwrap();

    assert_eq!(hashes.version, "2.8");
    assert!(hashes.device_public_key.is_some());
    assert!(hashes.backend_keys_json.len() == 64);
    assert!(hashes.di_iris_embeddings_shares_2_pb.is_some());
    assert!(hashes
        .right_normalized_mask_blinding_factors_resized_bin
        .is_some());
    assert!(hashes.right_depth_png.is_some());
}

#[test]
fn hashes_decodes_redacted_v2_7() {
    let hashes: Hashes =
        serde_json::from_str(&fixture("hashes_redacted_v2_7.json")).unwrap();

    assert_eq!(hashes.version, "2.7");
    assert!(hashes.device_public_key.is_none());
    assert!(hashes.iris_codes_json.is_none());
    assert!(hashes.left_ir_png.is_none());
}

#[test]
fn hashes_ignores_multiframe_keys() {
    let json =
        r#"{"version":"2.8","img-l1.png":"aa","img-l1_normalized_image.bin":"bb"}"#;

    let hashes: Hashes = serde_json::from_str(json).unwrap();

    assert_eq!(hashes.version, "2.8");
}

#[test]
fn hashes_serializes_with_file_names() {
    let hashes: Hashes = serde_json::from_str(&fixture("hashes.json")).unwrap();

    let value = serde_json::to_value(&hashes).unwrap();
    let expected: serde_json::Value =
        serde_json::from_str(&fixture("hashes.json")).unwrap();

    assert_eq!(value, expected);
}

#[test]
fn info_round_trips() {
    let raw = fixture("info.json");

    let info: Info = serde_json::from_str(&raw).unwrap();

    assert_eq!(info.left_ir_multiframe_image_ids, ["img-l1", "img-l2"]);
    assert_eq!(info.device_public_key.as_deref(), Some("BASE64KEY"));
    assert_eq!(
        serde_json::to_value(&info).unwrap(),
        serde_json::from_str::<serde_json::Value>(&raw).unwrap()
    );
}

#[test]
fn iris_codes_accepts_null_codes() {
    let codes: IrisCodes = serde_json::from_str(&fixture("iris_codes.json")).unwrap();

    assert_eq!(codes.iris_version.as_deref(), Some("1.6.1"));
    assert_eq!(codes.left_iris_code.as_deref(), Some("AAAA"));
    assert!(codes.right_iris_code.is_none());
}

#[test]
fn iris_code_shares_decodes() {
    let shares: IrisCodeShares =
        serde_json::from_str(&fixture("iris_code_shares_0.json")).unwrap();

    assert_eq!(shares.iris_shares_version, "c2d631d");
    assert_eq!(shares.right_mask_code_shares, "DD");
}

#[test]
fn backend_keys_decodes() {
    let keys: BackendKeys =
        serde_json::from_str(&fixture("backend_keys.json")).unwrap();

    assert_eq!(keys.tier2.unwrap().public_key, "p4");
}

#[test]
fn face_embeddings_decodes_as_array() {
    let embeddings: Vec<FaceEmbedding> =
        serde_json::from_str(&fixture("face_embeddings.json")).unwrap();

    assert_eq!(embeddings.len(), 1);
    assert_eq!(embeddings[0].embedding_inference_backend, "tensorrt");
}
