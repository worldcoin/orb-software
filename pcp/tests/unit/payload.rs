mod json {
    use crate::payload::{
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
}

mod di {
    use crate::payload::{self, DiEncodingError, DiEye};
    use orb_pcp_defs::{
        prost::Message,
        v1::{DiIrisEmbeddingShares, DiIrisEmbeddings},
    };

    fn left() -> DiEye<'static> {
        DiEye {
            model_version: "m",
            inference_backend: "b",
            embedding_version: "v",
            embedding: &[-128, 0, 127],
            mirror_embedding: &[-1],
            embedding_f32: &[1.0],
            mirror_embedding_f32: &[-0.0],
            embedding_shares: [&[0, 65535], &[3], &[7]],
            mirror_embedding_shares: [&[128], &[4], &[8]],
        }
    }

    fn right() -> DiEye<'static> {
        DiEye {
            embedding: &[1],
            mirror_embedding: &[-2],
            embedding_f32: &[2.0],
            mirror_embedding_f32: &[0.5],
            embedding_shares: [&[1], &[5], &[9]],
            mirror_embedding_shares: [&[2], &[6], &[10]],
            ..left()
        }
    }

    #[test]
    fn embeddings_have_exact_wire_bytes_and_preserve_signed_values() {
        let output = payload::encode_di(Some(&left()), Some(&right()), "s").unwrap();
        assert_eq!(
            output.embeddings,
            [
                0x0a, 0x31, 0x0a, 1, b'm', 0x12, 1, b'b', 0x1a, 1, b'v', 0x22, 5, 0xff,
                1, 0, 0xfe, 1, 0x2a, 1, 1, 0x32, 1, 2, 0x3a, 1, 3, 0x42, 4, 0, 0, 0x80,
                0x3f, 0x4a, 4, 0, 0, 0, 0x80, 0x52, 4, 0, 0, 0, 0x40, 0x5a, 4, 0, 0, 0,
                0x3f,
            ]
        );
        let decoded = DiIrisEmbeddings::decode(output.embeddings.as_slice())
            .unwrap()
            .embedding_v1
            .unwrap();
        assert_eq!(decoded.left_embedding, [-128, 0, 127]);
        assert_eq!(decoded.left_mirror_embedding, [-1]);
        assert_eq!(decoded.right_embedding, [1]);
        assert_eq!(decoded.right_mirror_embedding, [-2]);
        assert_eq!(
            decoded.left_mirror_embedding_f32[0].to_bits(),
            (-0.0_f32).to_bits()
        );
    }

    #[test]
    fn shares_preserve_unsigned_values_and_recipient_eye_mirror_association() {
        let left = left();
        let right = right();
        let output = payload::encode_di(Some(&left), Some(&right), "s").unwrap();
        assert_eq!(
            output.shares[0],
            [
                0x0a, 0x19, 0x0a, 1, b'm', 0x12, 1, b's', 0x1a, 1, b'v', 0x22, 4, 0,
                0xff, 0xff, 3, 0x2a, 2, 0x80, 1, 0x32, 1, 1, 0x3a, 1, 2,
            ]
        );
        for (i, bytes) in output.shares.iter().enumerate() {
            let decoded = DiIrisEmbeddingShares::decode(bytes.as_slice())
                .unwrap()
                .share_v1
                .unwrap();
            assert_eq!(decoded.model_version, "m");
            assert_eq!(decoded.embedding_version, "v");
            assert_eq!(decoded.shares_version, "s");
            let widen = |values: &[u16]| {
                values.iter().copied().map(u32::from).collect::<Vec<_>>()
            };
            assert_eq!(decoded.left_share, widen(left.embedding_shares[i]));
            assert_eq!(
                decoded.left_mirror_share,
                widen(left.mirror_embedding_shares[i])
            );
            assert_eq!(decoded.right_share, widen(right.embedding_shares[i]));
            assert_eq!(
                decoded.right_mirror_share,
                widen(right.mirror_embedding_shares[i])
            );
        }
    }

    #[test]
    fn either_missing_eye_produces_four_empty_buffers() {
        let eye = left();
        for (left, right) in [(None, None), (Some(&eye), None), (None, Some(&eye))] {
            let output = payload::encode_di(left, right, "s").unwrap();
            assert!(output.embeddings.is_empty());
            assert!(output.shares.iter().all(Vec::is_empty));
        }
    }

    #[test]
    fn metadata_mismatches_return_field_only_errors() {
        let left = left();
        for (field, expected) in [
            (0, DiEncodingError::ModelVersionMismatch),
            (1, DiEncodingError::InferenceBackendMismatch),
            (2, DiEncodingError::EmbeddingVersionMismatch),
        ] {
            let mut right = right();
            match field {
                0 => right.model_version = "sensitive-mismatched-value",
                1 => right.inference_backend = "sensitive-mismatched-value",
                _ => right.embedding_version = "sensitive-mismatched-value",
            }
            let error = payload::encode_di(Some(&left), Some(&right), "s")
                .err()
                .unwrap();
            assert!(!error.to_string().contains("sensitive-mismatched-value"));
            assert_eq!(error, expected);
        }
        let mut right = right();
        right.model_version = "different-model";
        right.inference_backend = "different-backend";
        right.embedding_version = "different-embedding";
        assert_eq!(
            payload::encode_di(Some(&left), Some(&right), "s").err(),
            Some(DiEncodingError::ModelVersionMismatch),
        );
        right.model_version = left.model_version;
        assert_eq!(
            payload::encode_di(Some(&left), Some(&right), "s").err(),
            Some(DiEncodingError::InferenceBackendMismatch),
        );
    }

    #[test]
    fn present_empty_vectors_are_not_absent_records() {
        let eye = DiEye {
            model_version: "",
            inference_backend: "",
            embedding_version: "",
            embedding: &[],
            mirror_embedding: &[],
            embedding_f32: &[],
            mirror_embedding_f32: &[],
            embedding_shares: [&[]; 3],
            mirror_embedding_shares: [&[]; 3],
        };
        let output = payload::encode_di(Some(&eye), Some(&eye), "").unwrap();
        assert_eq!(output.embeddings, [0x0a, 0]);
        assert!(output.shares.iter().all(|bytes| bytes == &[0x0a, 0]));
    }

    #[test]
    fn floats_are_preserved_bit_for_bit_without_normalization() {
        let values = [
            f32::from_bits(0x7fc0_1234),
            f32::INFINITY,
            f32::NEG_INFINITY,
            -0.0,
            f32::from_bits(1),
        ];
        let eye = DiEye {
            embedding_f32: &values,
            mirror_embedding_f32: &values,
            ..left()
        };
        let output = payload::encode_di(Some(&eye), Some(&eye), "s").unwrap();
        let decoded = DiIrisEmbeddings::decode(output.embeddings.as_slice())
            .unwrap()
            .embedding_v1
            .unwrap();
        for actual in [
            decoded.left_embedding_f32,
            decoded.left_mirror_embedding_f32,
            decoded.right_embedding_f32,
            decoded.right_mirror_embedding_f32,
        ] {
            assert_eq!(
                actual.iter().map(|f| f.to_bits()).collect::<Vec<_>>(),
                values.map(f32::to_bits)
            );
        }
    }
}
