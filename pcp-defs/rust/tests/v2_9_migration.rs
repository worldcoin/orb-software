use orb_pcp_defs::{
    prost::Message,
    v1::{Hashes, Info, Migration},
};
use serde_json::json;

const MIGRATION_JSON: &str = include_str!("fixtures/v2_9/migration.json");

#[test]
fn migration_round_trips_with_exact_json_field_names() {
    let migration: Migration = serde_json::from_str(MIGRATION_JSON).unwrap();
    assert_eq!(migration.source_pcp_version.as_deref(), Some("2.7"));
    assert_eq!(
        migration.src_signup_id.as_deref(),
        Some("test-previous-signup")
    );
    assert_eq!(migration.tee_version.as_deref(), Some("0.1.0-test"));
    assert_eq!(migration.migrated_ts, Some(1800000000));
    assert_eq!(
        migration.biometric_pipeline_version.as_deref(),
        Some("1.2.3-test")
    );

    let decoded = Migration::decode(migration.encode_to_vec().as_slice()).unwrap();
    assert_eq!(decoded, migration);
    assert_eq!(
        serde_json::to_value(&decoded).unwrap(),
        serde_json::from_str::<serde_json::Value>(MIGRATION_JSON).unwrap()
    );
}

#[test]
fn absent_null_and_present_empty_values_keep_presence() {
    for raw in [
        "{}",
        r#"{"tee_version":null,"migrated_ts":null,"biometric_pipeline_version":null}"#,
    ] {
        let migration: Migration = serde_json::from_str(raw).unwrap();
        assert_eq!(migration, Migration::default());
        assert_eq!(serde_json::to_value(migration).unwrap(), json!({}));
    }
    let migration = Migration {
        tee_version: Some(String::new()),
        biometric_pipeline_version: Some(String::new()),
        migrated_ts: Some(0),
        ..Default::default()
    };
    assert_eq!(
        Migration::decode(migration.encode_to_vec().as_slice()).unwrap(),
        migration
    );
    assert_eq!(
        serde_json::to_value(&migration).unwrap(),
        json!({"tee_version":"", "migrated_ts":"0", "biometric_pipeline_version":""})
    );
}

#[test]
fn timestamp_is_exact_at_uint64_limit_and_rejects_invalid_values() {
    let migration = Migration {
        migrated_ts: Some(u64::MAX),
        ..Default::default()
    };
    let json = serde_json::to_string(&migration).unwrap();
    assert_eq!(json, r#"{"migrated_ts":"18446744073709551615"}"#);
    assert_eq!(serde_json::from_str::<Migration>(&json).unwrap(), migration);
    assert_eq!(
        serde_json::from_str::<Migration>(r#"{"migrated_ts":1800000000}"#)
            .unwrap()
            .migrated_ts,
        Some(1800000000)
    );
    for raw in [
        r#"{"migrated_ts":"18446744073709551616"}"#,
        r#"{"migrated_ts":-1}"#,
        r#"{"migrated_ts":1.5}"#,
    ] {
        assert!(serde_json::from_str::<Migration>(raw).is_err());
    }
}

#[test]
fn migration_ignores_future_fields_using_shared_reader_policy() {
    let migration: Migration = serde_json::from_str(
        r#"{"future_field":true,"biometric_pipeline_version":"1.2.3"}"#,
    )
    .unwrap();
    assert_eq!(
        migration.biometric_pipeline_version.as_deref(),
        Some("1.2.3")
    );
}

#[test]
fn old_manifests_omit_migration_hashes_and_new_hashes_round_trip() {
    for old in [
        include_str!("fixtures/v2_8/hashes.json"),
        include_str!("fixtures/v2_8/hashes_redacted_v2_7.json"),
    ] {
        let hashes: Hashes = serde_json::from_str(old).unwrap();
        assert!(hashes.migration_json.is_none());
        assert!(hashes.legacy_tar.is_none());
        assert!(hashes.info_json.is_none());
        let json = serde_json::to_value(hashes).unwrap();
        assert!(json.get("migration.json").is_none());
        assert!(json.get("legacy.tar").is_none());
        assert!(json.get("info.json").is_none());
    }
    let expected = json!({
        "version":"2.9", "migration.json":"11".repeat(32),
        "legacy.tar":"22".repeat(32), "info.json":"33".repeat(32)
    });
    let hashes: Hashes = serde_json::from_value(expected.clone()).unwrap();
    assert_eq!(
        hashes.migration_json.as_deref(),
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
