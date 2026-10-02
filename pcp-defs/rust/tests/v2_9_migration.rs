use orb_pcp_defs::{
    prost::Message,
    v1::{Hashes, Migration, MigrationPipeline},
};
use serde_json::json;

const MIGRATION_JSON: &str = include_str!("fixtures/v2_9/migration.json");

#[test]
fn migration_round_trips_with_exact_json_field_names() {
    let migration: Migration = serde_json::from_str(MIGRATION_JSON).unwrap();
    assert_eq!(migration.source_pcp_version.as_deref(), Some("2.7"));
    assert_ne!(
        migration.orb_signup_id,
        migration.tee_generated_from_old_signup_id
    );
    assert_eq!(migration.pipeline.as_ref().unwrap().duration_ms, Some(123));

    let decoded = Migration::decode(migration.encode_to_vec().as_slice()).unwrap();
    assert_eq!(decoded, migration);
    assert_eq!(
        serde_json::to_value(&decoded).unwrap(),
        serde_json::from_str::<serde_json::Value>(MIGRATION_JSON).unwrap()
    );
}

#[test]
fn absent_null_and_present_empty_values_keep_presence() {
    for raw in ["{}", r#"{"migration_id":null,"pipeline":null}"#] {
        let migration: Migration = serde_json::from_str(raw).unwrap();
        assert_eq!(migration, Migration::default());
        assert_eq!(serde_json::to_value(migration).unwrap(), json!({}));
    }
    let migration = Migration {
        migration_id: Some(String::new()),
        pipeline: Some(MigrationPipeline {
            duration_ms: Some(0),
            ..Default::default()
        }),
        ..Default::default()
    };
    assert_eq!(
        Migration::decode(migration.encode_to_vec().as_slice()).unwrap(),
        migration
    );
    assert_eq!(
        serde_json::to_value(&migration).unwrap(),
        json!({"migration_id":"", "pipeline":{"duration_ms":"0"}})
    );
    let empty_pipeline: Migration = serde_json::from_str(r#"{"pipeline":{}}"#).unwrap();
    assert_eq!(empty_pipeline.pipeline, Some(MigrationPipeline::default()));
}

#[test]
fn duration_is_exact_at_uint64_limit_and_rejects_invalid_values() {
    let pipeline = MigrationPipeline {
        duration_ms: Some(u64::MAX),
        ..Default::default()
    };
    let json = serde_json::to_string(&pipeline).unwrap();
    assert_eq!(json, r#"{"duration_ms":"18446744073709551615"}"#);
    assert_eq!(
        serde_json::from_str::<MigrationPipeline>(&json).unwrap(),
        pipeline
    );
    for raw in [
        r#"{"duration_ms":"18446744073709551616"}"#,
        r#"{"duration_ms":-1}"#,
        r#"{"duration_ms":1.5}"#,
    ] {
        assert!(serde_json::from_str::<MigrationPipeline>(raw).is_err());
    }
}

#[test]
fn migration_ignores_future_fields_using_shared_reader_policy() {
    let migration: Migration = serde_json::from_str(
        r#"{"future_field":true,"pipeline":{"future_model":{},"iris_version":"v1"}}"#,
    )
    .unwrap();
    assert_eq!(
        migration.pipeline.unwrap().iris_version.as_deref(),
        Some("v1")
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
