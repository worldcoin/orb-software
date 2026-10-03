use orb_pcp_defs::{
    prost::Message,
    v1::{Hashes, Info, Migration},
};
use serde_json::json;

#[test]
fn migration_binary_round_trip_preserves_raw_digest() {
    let migration = Migration {
        tee_version: Some("0.1.0-test".into()),
        src_signup_id: Some("test-previous-signup".into()),
        source_pcp_version: Some("2.7".into()),
        source_hashes_sha256: Some((0x80..0xa0).collect()),
        migrated_ts: Some(1800000000),
        enclave_measurement: Some("test-measurement".into()),
        biometric_pipeline_version: Some("1.2.3-test".into()),
    };

    let wire = migration.encode_to_vec();
    assert_eq!(Migration::decode(wire.as_slice()).unwrap(), migration);
}

#[test]
fn binary_absent_and_present_empty_values_keep_presence() {
    let absent = Migration::decode(&[][..]).unwrap();
    assert_eq!(absent, Migration::default());
    assert!(absent.encode_to_vec().is_empty());

    let present = Migration {
        tee_version: Some(String::new()),
        source_hashes_sha256: Some(Vec::new()),
        biometric_pipeline_version: Some(String::new()),
        migrated_ts: Some(0),
        ..Default::default()
    };
    let wire = present.encode_to_vec();
    assert!(!wire.is_empty());
    assert_eq!(Migration::decode(wire.as_slice()).unwrap(), present);
}

#[test]
fn binary_timestamp_preserves_uint64_limit_and_rejects_truncation() {
    let migration = Migration {
        migrated_ts: Some(u64::MAX),
        ..Default::default()
    };
    let wire = migration.encode_to_vec();
    assert_eq!(Migration::decode(wire.as_slice()).unwrap(), migration);
    assert!(Migration::decode(&wire[..wire.len() - 1]).is_err());
}

#[test]
fn migration_binary_ignores_future_fields() {
    let migration = Migration {
        biometric_pipeline_version: Some("1.2.3".into()),
        ..Default::default()
    };
    let mut wire = migration.encode_to_vec();
    // Unknown field 8, varint value 1.
    wire.extend_from_slice(&[0x40, 0x01]);
    assert_eq!(Migration::decode(wire.as_slice()).unwrap(), migration);
}

#[test]
fn old_manifests_omit_migration_hashes_and_new_hashes_round_trip() {
    for old in [
        include_str!("fixtures/v2_8/hashes.json"),
        include_str!("fixtures/v2_8/hashes_redacted_v2_7.json"),
    ] {
        let hashes: Hashes = serde_json::from_str(old).unwrap();
        assert!(hashes.migration_pb.is_none());
        assert!(hashes.legacy_tar.is_none());
        assert!(hashes.info_json.is_none());
        let json = serde_json::to_value(hashes).unwrap();
        assert!(json.get("migration.pb").is_none());
        assert!(json.get("legacy.tar").is_none());
        assert!(json.get("info.json").is_none());
    }
    let expected = json!({
        "version":"2.9", "migration.pb":"11".repeat(32),
        "legacy.tar":"22".repeat(32), "info.json":"33".repeat(32)
    });
    let hashes: Hashes = serde_json::from_value(expected.clone()).unwrap();
    assert_eq!(
        hashes.migration_pb.as_deref(),
        Some("11".repeat(32).as_str())
    );
    assert_eq!(hashes.legacy_tar.as_deref(), Some("22".repeat(32).as_str()));
    assert_eq!(hashes.info_json.as_deref(), Some("33".repeat(32).as_str()));
    let decoded = Hashes::decode(hashes.encode_to_vec().as_slice()).unwrap();
    assert_eq!(decoded, hashes);
    assert_eq!(serde_json::to_value(decoded).unwrap(), expected);
}

#[test]
fn info_source_signup_is_optional_and_round_trips() {
    let old: Info =
        serde_json::from_str(include_str!("fixtures/v2_8/info.json")).unwrap();
    assert!(old.src_signup_id.is_none());
    assert!(serde_json::to_value(&old)
        .unwrap()
        .get("src_signup_id")
        .is_none());
    let absent: Info = serde_json::from_str(r#"{"src_signup_id":null}"#).unwrap();
    assert!(absent.src_signup_id.is_none());
    for source in ["", "test-previous-signup"] {
        let info = Info {
            signup_id: Some("test-new-signup".into()),
            src_signup_id: Some(source.into()),
            ..Default::default()
        };
        let encoded = serde_json::to_value(&info).unwrap();
        assert_eq!(encoded["src_signup_id"], source);
        assert!(encoded.get("srcSignupId").is_none());
        assert_eq!(serde_json::from_value::<Info>(encoded).unwrap(), info);
        assert_eq!(Info::decode(info.encode_to_vec().as_slice()).unwrap(), info);
    }
}
