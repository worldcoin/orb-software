use orb_pcp_defs::{prost::Message, v1::Migration};

use crate::migration::{LegacyArtifacts, MigrationProvenance};

fn legacy() -> LegacyArtifacts<'static> {
    LegacyArtifacts {
        hashes_json: b"old-hashes",
        hashes_sign: b"old-signature",
        face_embeddings_json: None,
        iris_codes_json: None,
        iris_code_shares_json: [None; 3],
        di_iris_embeddings_pb: None,
        di_iris_embeddings_shares_pb: [None; 3],
    }
}

fn provenance() -> MigrationProvenance<'static> {
    MigrationProvenance {
        tee_version: "tee",
        src_signup_id: "source-signup",
        source_pcp_version: "2.0",
        migrated_ts: u64::MAX,
        biometric_pipeline_version: "pipeline",
        legacy: legacy(),
    }
}

#[test]
fn protobuf_has_every_field_present_and_exact_bytes() {
    let bytes = provenance().encode();
    assert_eq!(
        Migration::decode(bytes.as_slice()).unwrap(),
        Migration {
            tee_version: Some("tee".into()),
            src_signup_id: Some("source-signup".into()),
            source_pcp_version: Some("2.0".into()),
            migrated_ts: Some(u64::MAX),
            biometric_pipeline_version: Some("pipeline".into()),
        }
    );
    let mut expected = b"\x0a\x03tee\x12\x0dsource-signup\x1a\x032.0\x28".to_vec();
    expected.extend([0xff; 9]);
    expected.extend(b"\x01\x3a\x08pipeline");
    assert_eq!(bytes, expected);
}

#[test]
fn minimal_legacy_inventory_is_the_source_manifest_and_signature() {
    let entries: Vec<_> = legacy().entries().collect();
    assert_eq!(
        entries,
        [
            ("legacy/hashes.sign", b"old-signature".as_slice()),
            ("legacy/hashes.json", b"old-hashes"),
        ]
    );
}

#[test]
fn each_empty_required_field_is_reported() {
    assert_eq!(provenance().empty_field(), None);
    for field in [
        "tee_version",
        "src_signup_id",
        "source_pcp_version",
        "biometric_pipeline_version",
        "legacy/hashes.json",
        "legacy/hashes.sign",
    ] {
        let mut input = provenance();
        match field {
            "tee_version" => input.tee_version = "",
            "src_signup_id" => input.src_signup_id = "",
            "source_pcp_version" => input.source_pcp_version = "",
            "biometric_pipeline_version" => input.biometric_pipeline_version = "",
            "legacy/hashes.json" => input.legacy.hashes_json = b"",
            _ => input.legacy.hashes_sign = b"",
        }
        assert_eq!(input.empty_field(), Some(field));
    }
}

#[test]
fn optional_legacy_files_may_be_empty_and_zero_timestamp_is_kept() {
    let mut input = provenance();
    input.migrated_ts = 0;
    input.legacy.di_iris_embeddings_pb = Some(b"");
    assert_eq!(input.empty_field(), None);
    assert!(input
        .legacy
        .entries()
        .any(|entry| entry == ("legacy/di_iris_embeddings.pb", b"".as_slice())));
    let decoded = Migration::decode(input.encode().as_slice()).unwrap();
    assert_eq!(decoded.migrated_ts, Some(0));
}
