use std::collections::BTreeSet;

use crate::metadata::{self, ImageIdPolicy};
use data_encoding::HEXLOWER;
use orb_pcp_defs::v1::Info;
use rand::{CryptoRng, RngCore, SeedableRng};

// Deterministic test RNG.
#[derive(Default)]
struct SaltRng {
    calls: usize,
    fail_at: Option<usize>,
}

impl CryptoRng for SaltRng {}

impl RngCore for SaltRng {
    fn next_u32(&mut self) -> u32 {
        panic!("use fallible byte generation")
    }
    fn next_u64(&mut self) -> u64 {
        panic!("use fallible byte generation")
    }
    fn fill_bytes(&mut self, _: &mut [u8]) {
        panic!("use fallible byte generation")
    }
    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand::Error> {
        assert_eq!(dest.len(), 16);
        let call = self.calls;
        self.calls += 1;
        if self.fail_at == Some(call) {
            return Err(rand::Error::new(std::io::Error::other(
                "synthetic entropy failure",
            )));
        }
        dest.fill(0xab);
        Ok(())
    }
}

fn info() -> Info {
    let some = |value: &str| Some(value.to_owned());
    Info {
        signup_id: some("signup"),
        signup_reason: some("reason"),
        orb_id: some("orb"),
        operator_id: some("operator"),
        timestamp: some("1"),
        qr_code: some("qr"),
        id_commitment: some("commitment"),
        software_version: some("software"),
        orb_country: some("country"),
        orb_public_key_certificate: some("AP8="),
        left_ir_image_id: some("left"),
        right_ir_image_id: some("right"),
        thumbnail_image_id: some("thumbnail"),
        left_iris_code_aggregate_image_ids: vec!["la".into()],
        right_iris_code_aggregate_image_ids: vec!["ra2".into(), "ra1".into()],
        ..Default::default()
    }
}

fn included() -> ImageIdPolicy<'static> {
    ImageIdPolicy::Included {
        left_multiframe: vec!["l2", "l1"],
        right_multiframe: vec![],
    }
}

fn json(bytes: &[u8]) -> serde_json::Value {
    serde_json::from_slice(bytes).unwrap()
}

#[test]
fn metadata_has_exact_sorted_bytes_and_salted_hash_coverage() {
    let mut rng = SaltRng::default();
    let encoded = metadata::encode(&info(), included(), &mut rng).unwrap();
    let salt = "abababababababababababababababab";
    let expected = format!(concat!(
        "{{\"id_commitment\":\"commitment\",\"id_commitment_salt\":\"{s}\",",
        "\"left_ir_image_id\":\"left\",\"left_ir_multiframe_image_ids\":[\"l2\",\"l1\"],",
        "\"left_iris_code_aggregate_image_ids\":[\"la\"],",
        "\"operator_id\":\"operator\",\"operator_id_salt\":\"{s}\",",
        "\"orb_country\":\"country\",\"orb_country_salt\":\"{s}\",",
        "\"orb_id\":\"orb\",\"orb_id_salt\":\"{s}\",\"orb_public_key_certificate\":\"AP8=\",",
        "\"qr_code\":\"qr\",\"qr_code_salt\":\"{s}\",",
        "\"right_ir_image_id\":\"right\",\"right_ir_multiframe_image_ids\":[],",
        "\"right_iris_code_aggregate_image_ids\":[\"ra2\",\"ra1\"],",
        "\"signup_id\":\"signup\",\"signup_id_salt\":\"{s}\",",
        "\"signup_reason\":\"reason\",\"signup_reason_salt\":\"{s}\",",
        "\"software_version\":\"software\",\"software_version_salt\":\"{s}\",",
        "\"thumbnail_image_id\":\"thumbnail\",\"timestamp\":\"1\",\"timestamp_salt\":\"{s}\"}}"
    ), s = salt);
    assert_eq!(encoded.info_json, expected.as_bytes());
    assert_eq!(rng.calls, 9);
    assert_eq!(encoded.hashes.len(), 9);
    for (name, value) in [
        ("signup_id", "signup"),
        ("signup_reason", "reason"),
        ("orb_id", "orb"),
        ("operator_id", "operator"),
        ("timestamp", "1"),
        ("qr_code", "qr"),
        ("id_commitment", "commitment"),
        ("software_version", "software"),
        ("orb_country", "country"),
    ] {
        let expected = ring::digest::digest(
            &ring::digest::SHA256,
            format!("{value}{salt}").as_bytes(),
        );
        assert_eq!(encoded.hashes[name].as_slice(), expected.as_ref());
    }
    assert_eq!(
        HEXLOWER.encode(&encoded.hashes["signup_id"]),
        "363845f1b33332b04503a29a2d73d9ec3bdc10d935f4c2a2c07bb3fbe7997f9c"
    );
}

#[test]
fn redaction_clears_all_image_ids_without_changing_identity_or_hashes() {
    let full = metadata::encode(&info(), included(), &mut SaltRng::default()).unwrap();
    let redacted =
        metadata::encode(&info(), ImageIdPolicy::Redacted, &mut SaltRng::default())
            .unwrap();
    let mut expected = json(&full.info_json);
    for key in [
        "left_ir_image_id",
        "right_ir_image_id",
        "thumbnail_image_id",
    ] {
        expected[key] = serde_json::json!("");
    }
    for key in [
        "left_ir_multiframe_image_ids",
        "right_ir_multiframe_image_ids",
        "left_iris_code_aggregate_image_ids",
        "right_iris_code_aggregate_image_ids",
    ] {
        expected[key] = serde_json::json!([]);
    }
    assert_eq!(json(&redacted.info_json), expected);
    assert_eq!(redacted.hashes, full.hashes);
}

#[test]
fn absent_fields_are_omitted_without_salt_or_hash() {
    let input = Info {
        signup_id: Some("signup".into()),
        ..Default::default()
    };
    let mut rng = SaltRng::default();
    let encoded = metadata::encode(&input, included(), &mut rng).unwrap();
    assert_eq!(rng.calls, 1);
    assert_eq!(
        encoded.hashes.keys().copied().collect::<Vec<_>>(),
        ["signup_id"]
    );
    assert_eq!(
        json(&encoded.info_json),
        serde_json::json!({
            "signup_id": "signup",
            "signup_id_salt": "abababababababababababababababab",
            "left_ir_multiframe_image_ids": ["l2", "l1"],
            "right_ir_multiframe_image_ids": [],
            "left_iris_code_aggregate_image_ids": [],
            "right_iris_code_aggregate_image_ids": [],
        })
    );
}

#[test]
fn caller_salts_and_multiframe_lists_are_replaced_and_other_fields_kept() {
    let mut input = info();
    input.signup_id_salt = Some("caller-salt".into());
    input.qr_code = None;
    input.qr_code_salt = Some("orphaned-salt".into());
    input.left_ir_multiframe_image_ids = vec!["stale".into()];
    input.right_ir_multiframe_image_ids = vec!["stale".into()];
    let encoded =
        metadata::encode(&input, included(), &mut SaltRng::default()).unwrap();
    let output: Info = serde_json::from_slice(&encoded.info_json).unwrap();
    assert_eq!(
        output.signup_id_salt.as_deref(),
        Some("abababababababababababababababab")
    );
    assert_eq!(output.qr_code, None);
    assert_eq!(output.qr_code_salt, None);
    assert!(!encoded.hashes.contains_key("qr_code"));
    assert_eq!(output.left_ir_multiframe_image_ids, ["l2", "l1"]);
    assert!(output.right_ir_multiframe_image_ids.is_empty());
    let mut expected = input;
    expected.qr_code_salt = None;
    expected.left_ir_multiframe_image_ids = output.left_ir_multiframe_image_ids.clone();
    expected.right_ir_multiframe_image_ids.clear();
    for (field, salt) in [
        (&mut expected.signup_id_salt, &output.signup_id_salt),
        (&mut expected.signup_reason_salt, &output.signup_reason_salt),
        (&mut expected.orb_id_salt, &output.orb_id_salt),
        (&mut expected.operator_id_salt, &output.operator_id_salt),
        (&mut expected.timestamp_salt, &output.timestamp_salt),
        (&mut expected.id_commitment_salt, &output.id_commitment_salt),
        (
            &mut expected.software_version_salt,
            &output.software_version_salt,
        ),
        (&mut expected.orb_country_salt, &output.orb_country_salt),
    ] {
        field.clone_from(salt);
    }
    assert_eq!(output, expected);
}

#[test]
fn device_key_presence_controls_value_salt_hash_and_randomness_together() {
    for key in [None, Some(""), Some("synthetic-device-key")] {
        let mut input = info();
        input.device_public_key = key.map(str::to_owned);
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        let encoded =
            metadata::encode(&input, ImageIdPolicy::Redacted, &mut rng).unwrap();
        let json = json(&encoded.info_json);
        assert_eq!(json.get("device_public_key").is_some(), key.is_some());
        assert_eq!(json.get("device_public_key").and_then(|v| v.as_str()), key);
        assert_eq!(json.get("device_public_key_salt").is_some(), key.is_some());
        assert_eq!(
            encoded.hashes.contains_key("device_public_key"),
            key.is_some()
        );
        let salts: BTreeSet<_> = json
            .as_object()
            .unwrap()
            .iter()
            .filter(|(name, _)| name.ends_with("_salt"))
            .map(|(_, value)| value.as_str().unwrap())
            .collect();
        assert_eq!(salts.len(), 9 + usize::from(key.is_some()));
        for salt in salts {
            assert_eq!(HEXLOWER.decode(salt.as_bytes()).unwrap().len(), 16);
        }
        for (name, hash) in &encoded.hashes {
            let value = json[name].as_str().unwrap();
            let salt = json[format!("{name}_salt")].as_str().unwrap();
            assert_eq!(
                hash.as_slice(),
                ring::digest::digest(
                    &ring::digest::SHA256,
                    format!("{value}{salt}").as_bytes()
                )
                .as_ref()
            );
        }
    }
}

#[test]
fn strings_are_json_escaped_and_hashed_unchanged() {
    let mut input = info();
    input.signup_id = Some("\"\\\n\0é".into());
    let encoded =
        metadata::encode(&input, ImageIdPolicy::Redacted, &mut SaltRng::default())
            .unwrap();
    let json = json(&encoded.info_json);
    assert_eq!(json["signup_id"], "\"\\\n\0é");
    let expected = ring::digest::digest(
        &ring::digest::SHA256,
        format!("\"\\\n\0é{}", json["signup_id_salt"].as_str().unwrap()).as_bytes(),
    );
    assert_eq!(encoded.hashes["signup_id"].as_slice(), expected.as_ref());
}

#[test]
fn entropy_failure_at_any_field_returns_no_result_and_does_not_retry() {
    let mut input = info();
    input.device_public_key = Some("synthetic-device-key".into());
    for fail_at in 0..10 {
        let mut rng = SaltRng {
            calls: 0,
            fail_at: Some(fail_at),
        };
        let result = metadata::encode(&input, ImageIdPolicy::Redacted, &mut rng);
        assert!(matches!(
            result,
            Err(metadata::MetadataError::Randomness(_))
        ));
        assert_eq!(rng.calls, fail_at + 1);
    }
}
