use orb_pcp_defs::{
    prost::Message,
    v1::{Hashes, Migration},
};
use serde_json::json;

#[test]
fn migration_binary_round_trip() {
    let migration = Migration {
        tee_version: Some("0.1.0-test".into()),
        src_signup_id: Some("test-previous-signup".into()),
        source_pcp_version: Some("2.7".into()),
        migrated_ts: Some(1800000000),
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
        src_signup_id: Some(String::new()),
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
fn migration_hash_json_uses_filename_and_omits_absent_field() {
    let hashes: Hashes = serde_json::from_value(json!({})).unwrap();
    assert!(hashes.migration_pb.is_none());
    assert!(serde_json::to_value(hashes)
        .unwrap()
        .get("migration.pb")
        .is_none());

    let expected = json!({"migration.pb": "11".repeat(32)});
    let hashes: Hashes = serde_json::from_value(expected.clone()).unwrap();
    assert_eq!(
        hashes.migration_pb.as_deref(),
        Some("11".repeat(32).as_str())
    );
    assert_eq!(serde_json::to_value(hashes).unwrap(), expected);
}
