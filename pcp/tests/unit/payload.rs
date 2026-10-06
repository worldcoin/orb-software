mod json {
    use crate::payload::{self, BackendKey, BackendKeys};
    use orb_pcp_defs::v1::{FaceEmbedding, IrisCodeShares, IrisCodes};

    fn shares() -> [IrisCodeShares; 3] {
        [
            ("li", "lm", "ri", "rm"),
            ("li1", "lm1", "ri1", "rm1"),
            ("li2", "lm2", "ri2", "rm2"),
        ]
        .map(|(li, lm, ri, rm)| IrisCodeShares {
            iris_version: None,
            iris_shares_version: Some("synthetic-sharing-v1".into()),
            left_iris_code_shares: Some(li.into()),
            left_mask_code_shares: Some(lm.into()),
            right_iris_code_shares: Some(ri.into()),
            right_mask_code_shares: Some(rm.into()),
        })
    }

    #[test]
    fn face_vectors_and_order_are_preserved_with_sorted_keys() {
        let embedding =
            |vector: &str, kind: &str, version: &str, backend: &str| FaceEmbedding {
                embedding: Some(vector.into()),
                embedding_type: Some(kind.into()),
                embedding_version: Some(version.into()),
                embedding_inference_backend: Some(backend.into()),
            };
        let embeddings = [
            embedding("second-vector", "type-b", "v2", "cpu-b"),
            embedding("first-vector", "type-a", "v1", "cpu-a"),
        ];
        assert_eq!(
            payload::face_embeddings(&embeddings).unwrap(),
            br#"[{"embedding":"second-vector","embedding_inference_backend":"cpu-b","embedding_type":"type-b","embedding_version":"v2"},{"embedding":"first-vector","embedding_inference_backend":"cpu-a","embedding_type":"type-a","embedding_version":"v1"}]"#,
        );
        assert_eq!(payload::face_embeddings(&[]).unwrap(), b"[]");
    }

    #[test]
    fn iris_codes_omit_missing_and_keep_empty_values() {
        let codes = IrisCodes {
            iris_version: Some("synthetic-version".into()),
            left_iris_code: Some("left".into()),
            left_mask_code: Some(String::new()),
            right_iris_code: None,
            right_mask_code: Some("right-mask".into()),
        };
        assert_eq!(
            payload::encode_daugman(&codes, &shares()).unwrap().codes,
            br#"{"IRIS_version":"synthetic-version","left_iris_code":"left","left_mask_code":"","right_mask_code":"right-mask"}"#,
        );
        assert_eq!(
            payload::encode_daugman(&IrisCodes::default(), &shares())
                .unwrap()
                .codes,
            br#"{}"#
        );
    }

    #[test]
    fn share_files_keep_recipient_order_and_exact_wire_names() {
        let encoded =
            payload::encode_daugman(&IrisCodes::default(), &shares()).unwrap();
        assert_eq!(
            encoded.shares[0],
            br#"{"IRIS_shares_version":"synthetic-sharing-v1","left_iris_code_shares":"li","left_mask_code_shares":"lm","right_iris_code_shares":"ri","right_mask_code_shares":"rm"}"#,
        );
        for (i, bytes) in encoded.shares.iter().enumerate() {
            let decoded: IrisCodeShares = serde_json::from_slice(bytes).unwrap();
            assert_eq!(decoded, shares()[i]);
        }
    }

    #[test]
    fn strings_are_json_escaped_without_interpreting_their_contents() {
        let codes = IrisCodes {
            iris_version: Some("\"\\\n\t\0é".into()),
            left_iris_code: Some("not base64".into()),
            ..Default::default()
        };
        assert_eq!(
            payload::encode_daugman(&codes, &shares()).unwrap().codes,
            "{\"IRIS_version\":\"\\\"\\\\\\n\\t\\u0000é\",\"left_iris_code\":\"not base64\"}".as_bytes(),
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
}

mod di {
    use crate::payload;
    use orb_pcp_defs::v1::{
        DiIrisEmbeddingShareV1, DiIrisEmbeddingShares, DiIrisEmbeddingV1,
        DiIrisEmbeddings,
    };

    #[test]
    fn messages_have_exact_wire_bytes_and_recipient_order() {
        let embeddings = DiIrisEmbeddings {
            embedding_v1: Some(DiIrisEmbeddingV1 {
                model_version: "m".into(),
                embedding_inference_backend: "b".into(),
                embedding_version: "v".into(),
                left_embedding: vec![-128, 0, 127],
                left_mirror_embedding: vec![-1],
                right_embedding: vec![1],
                right_mirror_embedding: vec![-2],
                left_embedding_f32: vec![1.0],
                left_mirror_embedding_f32: vec![-0.0],
                right_embedding_f32: vec![2.0],
                right_mirror_embedding_f32: vec![0.5],
            }),
        };
        let shares = [0, 1, 2].map(|i| DiIrisEmbeddingShares {
            share_v1: Some(DiIrisEmbeddingShareV1 {
                model_version: "m".into(),
                shares_version: "s".into(),
                embedding_version: "v".into(),
                left_share: vec![i],
                left_mirror_share: vec![],
                right_share: vec![],
                right_mirror_share: vec![],
            }),
        });
        let output = payload::encode_di(&embeddings, &shares);
        assert_eq!(
            output.embeddings,
            [
                0x0a, 0x31, 0x0a, 1, b'm', 0x12, 1, b'b', 0x1a, 1, b'v', 0x22, 5, 0xff,
                1, 0, 0xfe, 1, 0x2a, 1, 1, 0x32, 1, 2, 0x3a, 1, 3, 0x42, 4, 0, 0, 0x80,
                0x3f, 0x4a, 4, 0, 0, 0, 0x80, 0x52, 4, 0, 0, 0, 0x40, 0x5a, 4, 0, 0, 0,
                0x3f,
            ]
        );
        for (i, bytes) in output.shares.iter().enumerate() {
            assert_eq!(
                bytes,
                &[
                    0x0a, 0x0c, 0x0a, 1, b'm', 0x12, 1, b's', 0x1a, 1, b'v', 0x22, 1,
                    i as u8
                ]
            );
        }
    }

    #[test]
    fn default_messages_produce_empty_files() {
        let output = payload::encode_di(
            &DiIrisEmbeddings::default(),
            &std::array::from_fn(|_| DiIrisEmbeddingShares::default()),
        );
        assert!(output.embeddings.is_empty());
        assert!(output.shares.iter().all(Vec::is_empty));
    }
}
