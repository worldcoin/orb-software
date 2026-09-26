use std::{
    collections::BTreeSet,
    time::{Duration, UNIX_EPOCH},
};

use crate::metadata::{self, ImageIdPolicy, ImageIds, IrisImageIds, PackageInfo};
use data_encoding::HEXLOWER;
use rand::{CryptoRng, RngCore, SeedableRng};

// Deterministic test double only, never a production random source.
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

fn info() -> PackageInfo<'static> {
    PackageInfo {
        signup_id: "signup",
        signup_reason: "reason",
        orb_id: "orb",
        operator_id: "operator",
        capture_start: UNIX_EPOCH + Duration::from_millis(1999),
        qr_code: "qr",
        id_commitment: "commitment",
        software_version: "software",
        orb_country: "country",
        orb_public_key_certificate: &[0, 255],
        device_public_key: None,
    }
}

fn image_ids() -> ImageIds<'static> {
    ImageIds {
        left: Some(IrisImageIds {
            primary: "left",
            multiframe: &["l2", "l1"],
        }),
        right: Some(IrisImageIds {
            primary: "right",
            multiframe: &[],
        }),
        thumbnail: Some("thumbnail"),
        left_iris_code_aggregate: &["la"],
        right_iris_code_aggregate: &["ra2", "ra1"],
    }
}

#[test]
fn metadata_has_exact_sorted_bytes_and_salted_hash_coverage() {
    let mut rng = SaltRng::default();
    let encoded =
        metadata::encode(&info(), &ImageIdPolicy::Included(image_ids()), &mut rng)
            .unwrap();
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
    let full = metadata::encode(
        &info(),
        &ImageIdPolicy::Included(image_ids()),
        &mut SaltRng::default(),
    )
    .unwrap();
    let redacted =
        metadata::encode(&info(), &ImageIdPolicy::Redacted, &mut SaltRng::default())
            .unwrap();
    let mut expected: serde_json::Value =
        serde_json::from_slice(&full.info_json).unwrap();
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
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&redacted.info_json).unwrap(),
        expected
    );
    assert_eq!(redacted.hashes, full.hashes);
}

#[test]
fn device_key_presence_controls_value_salt_hash_and_randomness_together() {
    for key in [None, Some(""), Some("synthetic-device-key")] {
        let mut input = info();
        input.device_public_key = key;
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        let encoded =
            metadata::encode(&input, &ImageIdPolicy::Redacted, &mut rng).unwrap();
        let json: serde_json::Value =
            serde_json::from_slice(&encoded.info_json).unwrap();
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
fn strings_and_pre_epoch_capture_time_keep_legacy_encoding() {
    let mut input = info();
    input.signup_id = "\"\\\n\0é";
    for (time, seconds) in [
        (UNIX_EPOCH - Duration::from_secs(1), "0"),
        (UNIX_EPOCH, "0"),
    ] {
        input.capture_start = time;
        let encoded =
            metadata::encode(&input, &ImageIdPolicy::Redacted, &mut SaltRng::default())
                .unwrap();
        let json: serde_json::Value =
            serde_json::from_slice(&encoded.info_json).unwrap();
        assert_eq!(json["signup_id"], input.signup_id);
        assert_eq!(json["timestamp"], seconds);
        let expected = ring::digest::digest(
            &ring::digest::SHA256,
            format!(
                "{}{}",
                input.signup_id,
                json["signup_id_salt"].as_str().unwrap()
            )
            .as_bytes(),
        );
        assert_eq!(encoded.hashes["signup_id"].as_slice(), expected.as_ref());
    }
}

#[test]
fn missing_included_groups_fail_before_randomness_without_becoming_redacted() {
    for field in [
        "left_ir_image_id",
        "right_ir_image_id",
        "thumbnail_image_id",
    ] {
        let mut ids = image_ids();
        match field {
            "left_ir_image_id" => ids.left = None,
            "right_ir_image_id" => ids.right = None,
            _ => ids.thumbnail = None,
        }
        let mut rng = SaltRng::default();
        assert!(
            matches!(metadata::encode(&info(), &ImageIdPolicy::Included(ids), &mut rng),
            Err(metadata::MetadataError::MissingImageId { field: actual }) if actual == field)
        );
        assert_eq!(rng.calls, 0);
    }
}

#[test]
fn entropy_failure_at_any_field_returns_no_result_and_does_not_retry() {
    let mut input = info();
    input.device_public_key = Some("synthetic-device-key");
    for fail_at in 0..10 {
        let mut rng = SaltRng {
            calls: 0,
            fail_at: Some(fail_at),
        };
        let result = metadata::encode(&input, &ImageIdPolicy::Redacted, &mut rng);
        assert!(matches!(
            result,
            Err(metadata::MetadataError::Randomness(_))
        ));
        assert_eq!(rng.calls, fail_at + 1);
    }
}
