use orb_pcp_defs::v1::{
    BackendKeys, FaceEmbedding, Hashes, Info, IrisCodeShares, IrisCodes,
};
use serde::de::DeserializeOwned;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/v2_8");

fn decode<T: DeserializeOwned>(name: &str) -> T {
    let raw = std::fs::read_to_string(format!("{FIXTURES}/{name}"))
        .unwrap_or_else(|e| panic!("reading fixture {name}: {e}"));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("decoding {name}: {e}"))
}

#[test]
fn decodes_v2_8_files() {
    let hashes: Hashes = decode("hashes.json");
    assert_eq!(hashes.version.as_deref(), Some("2.8"));
    assert!(hashes.right_depth_png.is_some());

    let redacted: Hashes = decode("hashes_redacted_v2_7.json");
    assert_eq!(redacted.version.as_deref(), Some("2.7"));
    assert!(redacted.device_public_key.is_none());

    let info: Info = decode("info.json");
    assert_eq!(info.left_ir_multiframe_image_ids, ["img-l1", "img-l2"]);

    let codes: IrisCodes = decode("iris_codes.json");
    assert!(codes.right_iris_code.is_none());

    let shares: IrisCodeShares = decode("iris_code_shares_0.json");
    assert_eq!(shares.iris_shares_version.as_deref(), Some("c2d631d"));

    let keys: BackendKeys = decode("backend_keys.json");
    assert_eq!(keys.tier2.unwrap().public_key.as_deref(), Some("p4"));

    let embeddings: Vec<FaceEmbedding> = decode("face_embeddings.json");
    assert_eq!(embeddings.len(), 1);
}

#[test]
fn hashes_ignores_multiframe_keys() {
    let json =
        r#"{"version":"2.8","img-l1.png":"aa","img-l1_normalized_image.bin":"bb"}"#;

    let hashes: Hashes = serde_json::from_str(json).unwrap();

    assert_eq!(hashes.version.as_deref(), Some("2.8"));
}

#[test]
fn info_keeps_empty_arrays() {
    let info = Info {
        signup_id: Some("s".into()),
        ..Default::default()
    };

    let json = serde_json::to_value(&info).unwrap();

    assert_eq!(json["left_ir_multiframe_image_ids"], serde_json::json!([]));
    assert!(json.get("orb_id").is_none());
}
