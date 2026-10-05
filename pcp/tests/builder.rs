use std::time::UNIX_EPOCH;

use alkali::asymmetric::seal::curve25519xsalsa20poly1305 as sealedbox;
use orb_pcp::{
    self as pcp, BackendKey, BackendKeys, BiometricPolicy, BuildError, BuildRequest,
    MetadataError, PackageInfo, PcpVersion,
};
use rand::{CryptoRng, RngCore};

fn open(ciphertext: &[u8], pair: &sealedbox::Keypair) -> Vec<u8> {
    let mut plaintext = vec![0; ciphertext.len() - sealedbox::OVERHEAD_LENGTH];
    sealedbox::decrypt(ciphertext, pair, &mut plaintext).unwrap();
    plaintext
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[error("synthetic signing failure")]
struct SignerError;

fn request(key: &[u8; 32]) -> BuildRequest<'_> {
    let backend = || BackendKey {
        public_key: key,
        encrypted_private_key: "synthetic-envelope",
    };
    BuildRequest {
        version: PcpVersion::V2_8,
        timestamp: 1,
        info: PackageInfo {
            signup_id: "synthetic",
            signup_reason: "test",
            orb_id: "orb",
            operator_id: "operator",
            capture_start: UNIX_EPOCH,
            qr_code: Some("qr"),
            id_commitment: Some("id"),
            software_version: Some("test"),
            orb_country: Some("country"),
            orb_public_key_certificate: Some(b"synthetic-certificate"),
            device_public_key: Some("device"),
        },
        user_public_key: key,
        backend_keys: BackendKeys {
            iris: backend(),
            normalized_iris: backend(),
            face: backend(),
            tier2: backend(),
        },
        biometrics: BiometricPolicy::Redacted,
        migration: None,
    }
}

fn frame(id: &'static str) -> pcp::IrisFrame<'static> {
    pcp::IrisFrame {
        image_id: Some(id),
        ir_png: b"synthetic-ir",
        normalized: Some(pcp::NormalizedIrisFrame {
            image: b"image",
            mask: b"mask",
            image_resized: b"resized-image",
            mask_resized: b"resized-mask",
        }),
    }
}

fn images() -> pcp::PackageImages<'static> {
    pcp::PackageImages {
        left: Some(pcp::IrisEye {
            primary: frame("left"),
            multiframe: &[],
        }),
        right: Some(pcp::IrisEye {
            primary: frame("right"),
            multiframe: &[],
        }),
        thumbnail_png: Some(b"synthetic-thumbnail"),
        face_ir_png: Some(b"synthetic-face-ir"),
        thermal_png: Some(b"synthetic-thermal"),
        fraud: Some(pcp::FraudImages {
            scc_rgb_png: b"synthetic-scc",
            left_rgb_png: b"synthetic-left",
            right_rgb_png: b"synthetic-right",
            left_thermal_png: None,
            right_thermal_png: None,
            scc_depth_png: None,
            left_depth_png: None,
            right_depth_png: None,
        }),
    }
}

fn daugman() -> pcp::DaugmanData<'static> {
    let eye = || pcp::DaugmanEyeData {
        iris_code: None,
        mask_code: None,
        iris_code_shares: ["synthetic-share"; 3],
        mask_code_shares: ["synthetic-mask-share"; 3],
    };
    pcp::DaugmanData {
        iris_version: None,
        shares_version: "synthetic",
        left: eye(),
        right: eye(),
    }
}

fn included<'a>(
    images: &'a pcp::PackageImages<'a>,
    daugman: &'a pcp::DaugmanData<'a>,
) -> BiometricPolicy<'a> {
    BiometricPolicy::Included {
        images,
        thumbnail_image_id: Some("thumbnail"),
        left_iris_code_aggregate_image_ids: &[],
        right_iris_code_aggregate_image_ids: &[],
        face_embeddings: &[],
        daugman,
        di: None,
    }
}

fn provenance() -> pcp::MigrationProvenance<'static> {
    pcp::MigrationProvenance {
        tee_version: "synthetic-tee",
        src_signup_id: "synthetic-source",
        source_pcp_version: "2.0",
        migrated_ts: 1_800_000_000,
        biometric_pipeline_version: "synthetic-pipeline",
        legacy: pcp::LegacyArtifacts {
            hashes_json: b"{\"version\": \"2.0\"}\n",
            hashes_sign: b"synthetic-source-signature",
            face_embeddings_json: Some(b" [ ] "),
            iris_codes_json: Some(b"{\"left_iris_code\": \"old\"}"),
            iris_code_shares_json: [Some(b"old-share-0"), None, Some(b"")],
            di_iris_embeddings_pb: Some(b""),
            di_iris_embeddings_shares_pb: [None; 3],
        },
    }
}

fn must_not_sign(_: &[u8; 32]) -> Result<Vec<u8>, SignerError> {
    panic!("must not sign")
}

struct FailingRng;
impl CryptoRng for FailingRng {}
impl RngCore for FailingRng {
    fn next_u32(&mut self) -> u32 {
        panic!("unexpected randomness")
    }
    fn next_u64(&mut self) -> u64 {
        panic!("unexpected randomness")
    }
    fn fill_bytes(&mut self, _: &mut [u8]) {
        panic!("expected fallible randomness")
    }
    fn try_fill_bytes(&mut self, _: &mut [u8]) -> Result<(), rand::Error> {
        Err(rand::Error::new(std::io::Error::other(
            "synthetic entropy failure",
        )))
    }
}

#[test]
fn version_device_binding_mismatches_fail_before_signing() {
    for version in [PcpVersion::V2_7, PcpVersion::V2_8, PcpVersion::V2_9] {
        let mut input = request(&[0; 32]);
        input.version = version;
        if version != PcpVersion::V2_7 {
            input.info.device_public_key = None;
        }
        let result = pcp::build(
            &input,
            &mut FailingRng,
            |_| -> Result<Vec<u8>, SignerError> { panic!("must not sign") },
        );
        assert!(matches!(result, Err(BuildError::DeviceKeyVersionMismatch)));
    }
}

#[test]
fn version_device_matrix_preserves_metadata_and_manifest_contracts() {
    use std::io::Read;

    let pair = sealedbox::Keypair::generate().unwrap();
    for (version, label, device) in [
        (PcpVersion::V2_7, "2.7", None),
        (PcpVersion::V2_8, "2.8", Some("device")),
        (PcpVersion::V2_8, "2.8", Some("")),
        (PcpVersion::V2_9, "2.9", Some("device")),
        (PcpVersion::V2_9, "2.9", Some("")),
        (PcpVersion::V3_0, "3.0", None),
        (PcpVersion::V3_0, "3.0", Some("device")),
        (PcpVersion::V3_0, "3.0", Some("")),
    ] {
        let mut input = request(&pair.public_key);
        input.version = version;
        input.info.device_public_key = device;
        let output = pcp::build(&input, &mut rand::rngs::OsRng, |_| {
            Ok::<_, SignerError>(b"synthetic-signature".to_vec())
        })
        .unwrap();
        let gzip = open(&output.tier0, &pair);
        let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(&gzip[..]));
        let mut files = std::collections::BTreeMap::new();
        for entry in archive.entries().unwrap() {
            let mut entry = entry.unwrap();
            let name = entry.path().unwrap().into_owned();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            assert!(files.insert(name, bytes).is_none());
        }
        assert_eq!(files.len(), 4);
        let json = |name: &str| -> serde_json::Value {
            serde_json::from_slice(&files[std::path::Path::new(name)]).unwrap()
        };
        let info = json("info.json");
        let manifest = json("hashes.json");
        assert_eq!(manifest["version"], label);
        assert!(info.get("src_signup_id").is_none());
        assert!(manifest.get("migration.pb").is_none());
        assert_eq!(
            info.get("device_public_key").and_then(|x| x.as_str()),
            device
        );
        assert_eq!(
            info.get("device_public_key_salt").is_some(),
            device.is_some()
        );
        assert_eq!(
            manifest.get("device_public_key").is_some(),
            device.is_some()
        );
        for (name, bytes) in [("tier_1", &output.tier1), ("tier_2", &output.tier2)] {
            if label == "3.0" {
                let digest = ring::digest::digest(&ring::digest::SHA256, bytes);
                assert_eq!(
                    manifest[name],
                    data_encoding::HEXLOWER.encode(digest.as_ref())
                );
            } else {
                assert!(manifest.get(name).is_none());
            }
        }
        for name in ["tier_3", "tier_4", "tier_5"] {
            if label == "3.0" {
                assert_eq!(manifest[name], "0".repeat(64));
            } else {
                assert!(manifest.get(name).is_none());
            }
        }
    }
}

#[test]
fn invalid_user_key_fails_before_metadata_randomness_for_every_version() {
    for version in [
        PcpVersion::V2_7,
        PcpVersion::V2_8,
        PcpVersion::V2_9,
        PcpVersion::V3_0,
    ] {
        let mut input = request(&[0; 32]);
        input.version = version;
        input.info.device_public_key =
            (version != PcpVersion::V2_7).then_some("device");
        let result = pcp::build(
            &input,
            &mut FailingRng,
            |_| -> Result<Vec<u8>, SignerError> { panic!("must not sign") },
        );
        assert!(matches!(
            result,
            Err(BuildError::Encryption(pcp::SealingError::InvalidRecipient))
        ));
    }
}

#[test]
fn oversized_archive_timestamp_fails_before_signing() {
    let mut input = request(&[0; 32]);
    input.timestamp = u64::from(u32::MAX) + 1;
    let result = pcp::build(
        &input,
        &mut FailingRng,
        |_| -> Result<Vec<u8>, SignerError> { panic!("must not sign") },
    );
    assert!(matches!(
        result,
        Err(BuildError::Archive(pcp::ArchiveError::TimestampOutOfRange))
    ));
}

#[test]
fn randomness_failure_returns_no_package_or_signature() {
    let pair = sealedbox::Keypair::generate().unwrap();
    let result = pcp::build(
        &request(&pair.public_key),
        &mut FailingRng,
        |_| -> Result<Vec<u8>, SignerError> { panic!("must not sign") },
    );
    assert!(matches!(
        result,
        Err(BuildError::Metadata(MetadataError::Randomness(_)))
    ));
}

#[test]
fn invalid_user_recipient_returns_no_package_or_signature() {
    let result = pcp::build(
        &request(&[0; 32]),
        &mut rand::rngs::OsRng,
        |_| -> Result<Vec<u8>, SignerError> { panic!("must not sign") },
    );
    assert!(matches!(
        result,
        Err(BuildError::Encryption(pcp::SealingError::InvalidRecipient))
    ));
}

#[test]
fn signer_failure_is_not_retried_or_returned_as_a_package() {
    let pair = sealedbox::Keypair::generate().unwrap();
    let mut calls = 0;
    let result = pcp::build(&request(&pair.public_key), &mut rand::rngs::OsRng, |_| {
        calls += 1;
        Err::<Vec<u8>, _>(SignerError)
    });
    assert_eq!(calls, 1);
    assert!(matches!(
        result,
        Err(BuildError::Signing(pcp::SigningError::Signer(SignerError)))
    ));
}

#[test]
fn migration_requires_v2_9_before_signing() {
    let images = images();
    let daugman = daugman();
    for version in [PcpVersion::V2_7, PcpVersion::V2_8, PcpVersion::V3_0] {
        let mut input = request(&[0; 32]);
        input.version = version;
        input.info.device_public_key =
            (version != PcpVersion::V2_7).then_some("device");
        input.biometrics = included(&images, &daugman);
        input.migration = Some(provenance());
        let result = pcp::build(&input, &mut FailingRng, must_not_sign);
        assert!(matches!(result, Err(BuildError::MigrationVersionMismatch)));
    }
}

#[test]
fn v2_9_rejects_redaction_and_empty_provenance_before_signing() {
    let mut input = request(&[0; 32]);
    input.version = PcpVersion::V2_9;
    input.migration = Some(provenance());
    let result = pcp::build(&input, &mut FailingRng, must_not_sign);
    assert!(matches!(result, Err(BuildError::RedactedMigration)));

    let images = images();
    let daugman = daugman();
    input.biometrics = included(&images, &daugman);
    input.migration.as_mut().unwrap().legacy.hashes_sign = b"";
    let result = pcp::build(&input, &mut FailingRng, must_not_sign);
    assert!(matches!(
        result,
        Err(BuildError::EmptyMigrationField {
            field: "legacy/hashes.sign"
        })
    ));
}

#[test]
fn absent_capture_fields_are_rejected_outside_migrations_before_signing() {
    let daugman = daugman();
    for version in [
        PcpVersion::V2_7,
        PcpVersion::V2_8,
        PcpVersion::V2_9,
        PcpVersion::V3_0,
    ] {
        for field in [
            "qr_code",
            "id_commitment",
            "software_version",
            "orb_country",
            "orb_public_key_certificate",
            "left_ir_image_id",
            "right_ir_image_id",
            "thumbnail_image_id",
        ] {
            let mut images = images();
            match field {
                "left_ir_image_id" => {
                    images.left.as_mut().unwrap().primary.image_id = None
                }
                "right_ir_image_id" => {
                    images.right.as_mut().unwrap().primary.image_id = None
                }
                _ => {}
            }
            let mut input = request(&[0; 32]);
            input.version = version;
            input.info.device_public_key =
                (version != PcpVersion::V2_7).then_some("device");
            input.biometrics = included(&images, &daugman);
            match field {
                "qr_code" => input.info.qr_code = None,
                "id_commitment" => input.info.id_commitment = None,
                "software_version" => input.info.software_version = None,
                "orb_country" => input.info.orb_country = None,
                "orb_public_key_certificate" => {
                    input.info.orb_public_key_certificate = None
                }
                "thumbnail_image_id" => {
                    if let BiometricPolicy::Included {
                        thumbnail_image_id, ..
                    } = &mut input.biometrics
                    {
                        *thumbnail_image_id = None;
                    }
                }
                _ => {}
            }
            let result = pcp::build(&input, &mut FailingRng, must_not_sign);
            assert!(
                matches!(result, Err(BuildError::MissingRequiredField { field: actual }) if actual == field)
            );
        }
    }
}

#[test]
fn orb_v2_9_has_the_v2_8_layout_without_migration_files() {
    use std::io::Read;

    let pair = sealedbox::Keypair::generate().unwrap();
    let images = images();
    let daugman = daugman();
    let mut layouts = Vec::new();
    for version in [PcpVersion::V2_8, PcpVersion::V2_9] {
        let mut input = request(&pair.public_key);
        input.version = version;
        input.biometrics = included(&images, &daugman);
        let output = pcp::build(&input, &mut rand::rngs::OsRng, |_| {
            Ok::<_, SignerError>(b"synthetic-signature".to_vec())
        })
        .unwrap();
        let gzip = open(&output.tier0, &pair);
        let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(&gzip[..]));
        let mut names = Vec::new();
        let mut files = std::collections::BTreeMap::new();
        for entry in archive.entries().unwrap() {
            let mut entry = entry.unwrap();
            let name = entry.path().unwrap().to_str().unwrap().to_owned();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            names.push(name.clone());
            files.insert(name, bytes);
        }
        let manifest: serde_json::Value =
            serde_json::from_slice(&files["hashes.json"]).unwrap();
        let info: serde_json::Value =
            serde_json::from_slice(&files["info.json"]).unwrap();
        let mut keys: Vec<_> = manifest.as_object().unwrap().keys().cloned().collect();
        keys.retain(|key| key != "version");
        assert_eq!(
            manifest["version"],
            if version == PcpVersion::V2_8 {
                "2.8"
            } else {
                "2.9"
            }
        );
        assert!(info.get("src_signup_id").is_none());
        layouts.push((names, keys));
    }
    assert_eq!(layouts[0], layouts[1]);
    assert!(!layouts[0]
        .0
        .iter()
        .any(|name| name == "migration.pb" || name.starts_with("legacy/")));
}

#[test]
fn v2_9_round_trip_preserves_legacy_bytes_and_covers_migration() {
    use orb_pcp_defs::{prost::Message, v1};
    use std::io::Read;

    let sha256 = |bytes: &[u8]| {
        data_encoding::HEXLOWER
            .encode(ring::digest::digest(&ring::digest::SHA256, bytes).as_ref())
    };
    let pair = sealedbox::Keypair::generate().unwrap();
    let daugman = daugman();
    for (device, sparse) in [
        (None, false),
        (Some("device"), false),
        (None, true),
        (Some("device"), true),
    ] {
        let mut images = images();
        if sparse {
            for eye in [&mut images.left, &mut images.right] {
                eye.as_mut().unwrap().primary.image_id = None;
            }
        }
        let mut input = request(&pair.public_key);
        input.version = PcpVersion::V2_9;
        input.info.device_public_key = device;
        input.biometrics = included(&images, &daugman);
        input.migration = Some(provenance());
        if sparse {
            input.info.qr_code = None;
            input.info.id_commitment = None;
            input.info.software_version = None;
            input.info.orb_country = None;
            input.info.orb_public_key_certificate = None;
            if let BiometricPolicy::Included {
                thumbnail_image_id, ..
            } = &mut input.biometrics
            {
                *thumbnail_image_id = None;
            }
        }
        let mut signed = None;
        let output = pcp::build(&input, &mut rand::rngs::OsRng, |digest| {
            signed = Some(*digest);
            Ok::<_, SignerError>(b"synthetic-signature".to_vec())
        })
        .unwrap();

        let gzip = open(&output.tier0, &pair);
        let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(&gzip[..]));
        let mut entries = Vec::new();
        for entry in archive.entries().unwrap() {
            let mut entry = entry.unwrap();
            let name = entry.path().unwrap().to_str().unwrap().to_owned();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            entries.push((name, bytes));
        }
        let names: Vec<_> = entries.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(
            names,
            [
                "iris.tar",
                "normalized_iris.tar",
                "face.tar",
                "fraud.tar",
                "face_ir_and_thermal.tar",
                "info.json",
                "migration.pb",
                "face_embeddings.json",
                "iris_codes.json",
                "iris_code_shares_0.json",
                "iris_code_shares_1.json",
                "iris_code_shares_2.json",
                "di_iris_embeddings.pb",
                "di_iris_embeddings_shares_0.pb",
                "di_iris_embeddings_shares_1.pb",
                "di_iris_embeddings_shares_2.pb",
                "legacy/face_embeddings.json",
                "legacy/iris_codes.json",
                "legacy/iris_code_shares_0.json",
                "legacy/iris_code_shares_2.json",
                "legacy/di_iris_embeddings.pb",
                "legacy/hashes.sign",
                "legacy/hashes.json",
                "hashes.sign",
                "hashes.json",
                "backend_keys.json",
            ]
        );
        let files: std::collections::BTreeMap<_, _> = entries.into_iter().collect();
        for (name, expected) in [
            ("legacy/face_embeddings.json", b" [ ] ".as_slice()),
            ("legacy/iris_codes.json", b"{\"left_iris_code\": \"old\"}"),
            ("legacy/iris_code_shares_0.json", b"old-share-0"),
            ("legacy/iris_code_shares_2.json", b""),
            ("legacy/di_iris_embeddings.pb", b""),
            ("legacy/hashes.sign", b"synthetic-source-signature"),
            ("legacy/hashes.json", b"{\"version\": \"2.0\"}\n"),
        ] {
            assert_eq!(files[name], expected, "{name} changed");
        }
        for tier in [&output.tier1, &output.tier2] {
            let mut tar = Vec::new();
            flate2::read::GzDecoder::new(open(tier, &pair).as_slice())
                .read_to_end(&mut tar)
                .unwrap();
            assert_eq!(tar, [0; 1024]);
        }

        let digest = ring::digest::digest(&ring::digest::SHA256, &files["hashes.json"]);
        assert_eq!(signed.as_ref().map(|x| x.as_slice()), Some(digest.as_ref()));
        assert_eq!(
            v1::Migration::decode(files["migration.pb"].as_slice()).unwrap(),
            v1::Migration {
                tee_version: Some("synthetic-tee".into()),
                src_signup_id: Some("synthetic-source".into()),
                source_pcp_version: Some("2.0".into()),
                migrated_ts: Some(1_800_000_000),
                biometric_pipeline_version: Some("synthetic-pipeline".into()),
            }
        );
        let manifest: serde_json::Value =
            serde_json::from_slice(&files["hashes.json"]).unwrap();
        let manifest = manifest.as_object().unwrap();
        assert_eq!(manifest["version"], "2.9");
        assert_eq!(manifest["migration.pb"], sha256(&files["migration.pb"]));
        assert!(manifest
            .keys()
            .all(|name| !name.starts_with("legacy/") && !name.starts_with("tier_")));
        let hashes: v1::Hashes = serde_json::from_slice(&files["hashes.json"]).unwrap();
        assert_eq!(
            hashes.migration_pb.as_deref(),
            manifest["migration.pb"].as_str()
        );

        let raw_info: serde_json::Value =
            serde_json::from_slice(&files["info.json"]).unwrap();
        let info: v1::Info = serde_json::from_slice(&files["info.json"]).unwrap();
        assert_eq!(info.signup_id.as_deref(), Some("synthetic"));
        assert_eq!(info.src_signup_id.as_deref(), Some("synthetic-source"));
        assert!(raw_info.get("src_signup_id_salt").is_none());
        assert!(!manifest.contains_key("src_signup_id"));
        assert_eq!(info.device_public_key.as_deref(), device);
        assert_eq!(manifest.contains_key("device_public_key"), device.is_some());
        for (field, value) in [
            ("qr_code", info.qr_code.as_deref()),
            ("id_commitment", info.id_commitment.as_deref()),
            ("software_version", info.software_version.as_deref()),
            ("orb_country", info.orb_country.as_deref()),
        ] {
            assert_eq!(value.is_none(), sparse, "{field}");
            assert_eq!(raw_info.get(format!("{field}_salt")).is_none(), sparse);
            assert_eq!(manifest.contains_key(field), !sparse, "{field}");
        }
        for (field, value) in [
            (
                "orb_public_key_certificate",
                info.orb_public_key_certificate.as_deref(),
            ),
            ("left_ir_image_id", info.left_ir_image_id.as_deref()),
            ("right_ir_image_id", info.right_ir_image_id.as_deref()),
            ("thumbnail_image_id", info.thumbnail_image_id.as_deref()),
        ] {
            assert_eq!(value.is_none(), sparse, "{field}");
            assert_eq!(raw_info.get(field).is_none(), sparse, "{field}");
        }
    }
}

#[cfg(feature = "not-prod-diagnostics")]
mod diagnostics {
    use super::*;
    use std::{collections::BTreeMap, io::Read};

    fn entries(tar: &[u8]) -> BTreeMap<String, Vec<u8>> {
        tar::Archive::new(tar)
            .entries()
            .unwrap()
            .map(|entry| {
                let mut entry = entry.unwrap();
                let name = entry.path().unwrap().to_str().unwrap().to_owned();
                let mut bytes = Vec::new();
                entry.read_to_end(&mut bytes).unwrap();
                (name, bytes)
            })
            .collect()
    }

    fn tier(gzip: &[u8]) -> BTreeMap<String, Vec<u8>> {
        let mut tar = Vec::new();
        flate2::read::GzDecoder::new(gzip)
            .read_to_end(&mut tar)
            .unwrap();
        entries(&tar)
    }

    fn hash(bytes: &[u8]) -> String {
        data_encoding::HEXLOWER
            .encode(ring::digest::digest(&ring::digest::SHA256, bytes).as_ref())
    }

    #[test]
    fn diagnostic_redacted_tiers_are_gzip_for_all_versions() {
        for version in [
            PcpVersion::V2_7,
            PcpVersion::V2_8,
            PcpVersion::V2_9,
            PcpVersion::V3_0,
        ] {
            let mut input = request(&[0; 32]);
            input.version = version;
            input.info.device_public_key =
                (version != PcpVersion::V2_7).then_some("device");
            let mut calls = 0;
            let output: pcp::DiagnosticPackage =
                pcp::build_unencrypted_for_diagnostics(
                    &input,
                    &mut rand::rngs::OsRng,
                    |_| {
                        calls += 1;
                        Ok::<_, SignerError>(b"synthetic-signature".to_vec())
                    },
                )
                .unwrap();
            assert_eq!(calls, 1);
            let tier0 = tier(&output.tier0);
            assert_eq!(tier0.len(), 4);
            assert!(tier(&output.tier1).is_empty());
            assert!(tier(&output.tier2).is_empty());
            let manifest: serde_json::Value =
                serde_json::from_slice(&tier0["hashes.json"]).unwrap();
            assert_eq!(
                manifest["version"],
                match version {
                    PcpVersion::V2_7 => "2.7",
                    PcpVersion::V2_8 => "2.8",
                    PcpVersion::V2_9 => "2.9",
                    PcpVersion::V3_0 => "3.0",
                }
            );
            if version == PcpVersion::V3_0 {
                assert_eq!(manifest["tier_1"], hash(&output.tier1));
                assert_eq!(manifest["tier_2"], hash(&output.tier2));
            }
        }
    }

    #[test]
    fn diagnostic_included_inner_archives_are_plaintext_for_all_versions() {
        let images = images();
        let daugman = daugman();
        for version in [
            PcpVersion::V2_7,
            PcpVersion::V2_8,
            PcpVersion::V2_9,
            PcpVersion::V3_0,
        ] {
            let mut input = request(&[0; 32]);
            input.version = version;
            input.info.device_public_key =
                (version != PcpVersion::V2_7).then_some("device");
            input.biometrics = included(&images, &daugman);
            input.migration = (version == PcpVersion::V2_9).then(provenance);
            let output = pcp::build_unencrypted_for_diagnostics(
                &input,
                &mut rand::rngs::OsRng,
                |_| Ok::<_, SignerError>(Vec::new()),
            )
            .unwrap();
            let tiers = [&output.tier0, &output.tier1, &output.tier2].map(|x| tier(x));
            let archives = &tiers[usize::from(version == PcpVersion::V3_0)];
            assert_eq!(
                entries(&archives["iris.tar"])["left_ir.png"],
                b"synthetic-ir"
            );
            assert_eq!(
                entries(&archives["normalized_iris.tar"])["left_normalized_image.bin"],
                b"image"
            );
            assert_eq!(
                entries(&archives["face.tar"])["thumbnail.png"],
                b"synthetic-thumbnail"
            );
            assert_eq!(
                entries(&archives["fraud.tar"])["scc_rgb.png"],
                b"synthetic-scc"
            );
            let modalities = &tiers[if version == PcpVersion::V3_0 { 2 } else { 0 }];
            let modalities = entries(&modalities["face_ir_and_thermal.tar"]);
            assert_eq!(modalities["face_ir.png"], b"synthetic-face-ir");
            assert_eq!(modalities["thermal.png"], b"synthetic-thermal");
        }
    }

    #[test]
    fn normal_builder_remains_encrypted_with_diagnostics_enabled() {
        let pair = sealedbox::Keypair::generate().unwrap();
        let output =
            pcp::build(&request(&pair.public_key), &mut rand::rngs::OsRng, |_| {
                Ok::<_, SignerError>(Vec::new())
            })
            .unwrap();
        for encrypted in [&output.tier0, &output.tier1, &output.tier2] {
            let gzip = open(encrypted, &pair);
            tier(&gzip);
        }
    }
}
