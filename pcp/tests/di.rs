use orb_pcp::di::{self, Error, Eye};
use orb_pcp_defs::{
    prost::Message,
    v1::{DiIrisEmbeddingShares, DiIrisEmbeddings},
};

fn left() -> Eye<'static> {
    Eye {
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

fn right() -> Eye<'static> {
    Eye {
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
    let output = di::encode(Some(&left()), Some(&right()), "s").unwrap();
    assert_eq!(
        output.embeddings,
        [
            0x0a, 0x31, 0x0a, 1, b'm', 0x12, 1, b'b', 0x1a, 1, b'v', 0x22, 5, 0xff, 1,
            0, 0xfe, 1, 0x2a, 1, 1, 0x32, 1, 2, 0x3a, 1, 3, 0x42, 4, 0, 0, 0x80, 0x3f,
            0x4a, 4, 0, 0, 0, 0x80, 0x52, 4, 0, 0, 0, 0x40, 0x5a, 4, 0, 0, 0, 0x3f,
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
    let output = di::encode(Some(&left), Some(&right), "s").unwrap();
    assert_eq!(
        output.shares[0],
        [
            0x0a, 0x19, 0x0a, 1, b'm', 0x12, 1, b's', 0x1a, 1, b'v', 0x22, 4, 0, 0xff,
            0xff, 3, 0x2a, 2, 0x80, 1, 0x32, 1, 1, 0x3a, 1, 2,
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
        let widen =
            |values: &[u16]| values.iter().copied().map(u32::from).collect::<Vec<_>>();
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
        let output = di::encode(left, right, "s").unwrap();
        assert!(output.embeddings.is_empty());
        assert!(output.shares.iter().all(Vec::is_empty));
    }
}

#[test]
fn metadata_mismatches_return_field_only_errors() {
    let left = left();
    for (field, expected) in [
        (0, Error::ModelVersionMismatch),
        (1, Error::InferenceBackendMismatch),
        (2, Error::EmbeddingVersionMismatch),
    ] {
        let mut right = right();
        match field {
            0 => right.model_version = "sensitive-mismatched-value",
            1 => right.inference_backend = "sensitive-mismatched-value",
            _ => right.embedding_version = "sensitive-mismatched-value",
        }
        let error = di::encode(Some(&left), Some(&right), "s").err().unwrap();
        assert!(!error.to_string().contains("sensitive-mismatched-value"));
        assert_eq!(error, expected);
    }
    let mut right = right();
    right.model_version = "different-model";
    right.inference_backend = "different-backend";
    right.embedding_version = "different-embedding";
    assert_eq!(
        di::encode(Some(&left), Some(&right), "s").err(),
        Some(Error::ModelVersionMismatch),
    );
    right.model_version = left.model_version;
    assert_eq!(
        di::encode(Some(&left), Some(&right), "s").err(),
        Some(Error::InferenceBackendMismatch),
    );
}

#[test]
fn present_empty_vectors_are_not_absent_records() {
    let eye = Eye {
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
    let output = di::encode(Some(&eye), Some(&eye), "").unwrap();
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
    let eye = Eye {
        embedding_f32: &values,
        mirror_embedding_f32: &values,
        ..left()
    };
    let output = di::encode(Some(&eye), Some(&eye), "s").unwrap();
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
