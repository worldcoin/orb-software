use orb_pcp::payload::{
    self, BackendKey, BackendKeys, FaceEmbedding, IrisCodeShare, IrisCodes,
};

#[test]
fn face_vectors_and_order_are_preserved_with_sorted_keys() {
    let embeddings = [
        FaceEmbedding {
            embedding: "second-vector",
            embedding_type: "type-b",
            embedding_version: "v2",
            embedding_inference_backend: "cpu-b",
        },
        FaceEmbedding {
            embedding: "first-vector",
            embedding_type: "type-a",
            embedding_version: "v1",
            embedding_inference_backend: "cpu-a",
        },
    ];
    assert_eq!(
        payload::face_embeddings(&embeddings).unwrap(),
        br#"[{"embedding":"second-vector","embedding_inference_backend":"cpu-b","embedding_type":"type-b","embedding_version":"v2"},{"embedding":"first-vector","embedding_inference_backend":"cpu-a","embedding_type":"type-a","embedding_version":"v1"}]"#,
    );
    assert_eq!(payload::face_embeddings(&[]).unwrap(), b"[]");
}

#[test]
fn iris_codes_preserve_missing_and_empty_values() {
    let codes = IrisCodes {
        iris_version: Some("synthetic-version"),
        left_iris_code: Some("left"),
        left_mask_code: Some(""),
        right_iris_code: None,
        right_mask_code: Some("right-mask"),
    };
    assert_eq!(
        payload::iris_codes(&codes).unwrap(),
        br#"{"IRIS_version":"synthetic-version","left_iris_code":"left","left_mask_code":"","right_iris_code":null,"right_mask_code":"right-mask"}"#,
    );
    let missing = IrisCodes {
        iris_version: None,
        left_iris_code: None,
        left_mask_code: None,
        right_iris_code: None,
        right_mask_code: None,
    };
    assert_eq!(
        payload::iris_codes(&missing).unwrap(),
        br#"{"IRIS_version":null,"left_iris_code":null,"left_mask_code":null,"right_iris_code":null,"right_mask_code":null}"#,
    );
}

#[test]
fn share_fields_and_versions_use_exact_wire_names() {
    let mut share = IrisCodeShare {
        iris_version: None,
        iris_shares_version: "synthetic-sharing-v1",
        left_iris_code_shares: "li",
        left_mask_code_shares: "lm",
        right_iris_code_shares: "ri",
        right_mask_code_shares: "rm",
    };
    assert_eq!(
        payload::iris_code_share(&share).unwrap(),
        br#"{"IRIS_shares_version":"synthetic-sharing-v1","IRIS_version":null,"left_iris_code_shares":"li","left_mask_code_shares":"lm","right_iris_code_shares":"ri","right_mask_code_shares":"rm"}"#,
    );
    share.iris_version = Some("synthetic-iris-v1");
    let value: serde_json::Value =
        serde_json::from_slice(&payload::iris_code_share(&share).unwrap()).unwrap();
    assert_eq!(value["IRIS_version"], "synthetic-iris-v1");
}

#[test]
fn strings_are_json_escaped_without_interpreting_their_contents() {
    let codes = IrisCodes {
        iris_version: Some("\"\\\n\t\0é"),
        left_iris_code: Some("not base64"),
        left_mask_code: None,
        right_iris_code: None,
        right_mask_code: None,
    };
    assert_eq!(
        payload::iris_codes(&codes).unwrap(),
        "{\"IRIS_version\":\"\\\"\\\\\\n\\t\\u0000é\",\"left_iris_code\":\"not base64\",\"left_mask_code\":null,\"right_iris_code\":null,\"right_mask_code\":null}".as_bytes(),
    );
}

#[test]
fn backend_roles_and_nested_keys_are_sorted_with_padded_base64() {
    let keys = BackendKeys {
        iris: BackendKey {
            public_key: &[0; 32],
            encrypted_private_key: "iris-envelope",
        },
        normalized_iris: BackendKey {
            public_key: &[255; 32],
            encrypted_private_key: "normalized-envelope",
        },
        face: BackendKey {
            public_key: &[251; 32],
            encrypted_private_key: "face-envelope",
        },
        tier2: BackendKey {
            public_key: &[1; 32],
            encrypted_private_key: "tier2-envelope",
        },
    };
    assert_eq!(
        payload::backend_keys(&keys).unwrap(),
        br#"{"face":{"encrypted_private_key":"face-envelope","public_key":"+/v7+/v7+/v7+/v7+/v7+/v7+/v7+/v7+/v7+/v7+/s="},"iris":{"encrypted_private_key":"iris-envelope","public_key":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="},"normalized_iris":{"encrypted_private_key":"normalized-envelope","public_key":"//////////////////////////////////////////8="},"tier2":{"encrypted_private_key":"tier2-envelope","public_key":"AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE="}}"#,
    );
}
