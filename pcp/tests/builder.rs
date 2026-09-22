use std::time::UNIX_EPOCH;

use orb_pcp::{
    self as pcp, BackendKey, BackendKeys, BiometricPolicy, BuildError, BuildRequest,
    MetadataError, PackageInfo, PcpVersion,
};
use rand::{CryptoRng, RngCore};
use sodiumoxide::crypto::box_;

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
            qr_code: "qr",
            id_commitment: "id",
            software_version: "test",
            orb_country: "country",
            orb_public_key_certificate: b"synthetic-certificate",
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
    }
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
    for version in [PcpVersion::V2_7, PcpVersion::V2_8] {
        let mut input = request(&[0; 32]);
        input.version = version;
        if version == PcpVersion::V2_8 {
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

    sodiumoxide::init().unwrap();
    let (public, secret) = box_::gen_keypair();
    for (version, label, device) in [
        (PcpVersion::V2_7, "2.7", None),
        (PcpVersion::V2_8, "2.8", Some("device")),
        (PcpVersion::V2_8, "2.8", Some("")),
        (PcpVersion::V3_0, "3.0", None),
        (PcpVersion::V3_0, "3.0", Some("device")),
        (PcpVersion::V3_0, "3.0", Some("")),
    ] {
        let mut input = request(&public.0);
        input.version = version;
        input.info.device_public_key = device;
        let output = pcp::build(&input, &mut rand::rngs::OsRng, |_| {
            Ok::<_, SignerError>(b"synthetic-signature".to_vec())
        })
        .unwrap();
        let gzip =
            sodiumoxide::crypto::sealedbox::open(&output.tier0, &public, &secret)
                .unwrap();
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
    for version in [PcpVersion::V2_7, PcpVersion::V2_8, PcpVersion::V3_0] {
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
    sodiumoxide::init().unwrap();
    let (key, _) = box_::gen_keypair();
    let result = pcp::build(
        &request(&key.0),
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
    sodiumoxide::init().unwrap();
    let (key, _) = box_::gen_keypair();
    let mut calls = 0;
    let result = pcp::build(&request(&key.0), &mut rand::rngs::OsRng, |_| {
        calls += 1;
        Err::<Vec<u8>, _>(SignerError)
    });
    assert_eq!(calls, 1);
    assert!(matches!(
        result,
        Err(BuildError::Signing(pcp::SigningError::Signer(SignerError)))
    ));
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
        for version in [PcpVersion::V2_7, PcpVersion::V2_8, PcpVersion::V3_0] {
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
        let frame = |id| pcp::IrisFrame {
            image_id: id,
            ir_png: b"synthetic-ir",
            normalized: Some(pcp::NormalizedIrisFrame {
                image: b"image",
                mask: b"mask",
                image_resized: b"resized-image",
                mask_resized: b"resized-mask",
            }),
        };
        let images = pcp::PackageImages {
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
        };
        let eye = || pcp::IrisEyeData {
            iris_code: None,
            mask_code: None,
            iris_code_shares: ["synthetic-share"; 3],
            mask_code_shares: ["synthetic-mask-share"; 3],
        };
        let iris = pcp::IrisData {
            iris_version: None,
            shares_version: "synthetic",
            left: eye(),
            right: eye(),
        };
        for version in [PcpVersion::V2_7, PcpVersion::V2_8, PcpVersion::V3_0] {
            let mut input = request(&[0; 32]);
            input.version = version;
            input.info.device_public_key =
                (version != PcpVersion::V2_7).then_some("device");
            input.biometrics = BiometricPolicy::Included {
                images: &images,
                thumbnail_image_id: Some("thumbnail"),
                left_iris_code_aggregate_image_ids: &[],
                right_iris_code_aggregate_image_ids: &[],
                face_embeddings: &[],
                iris: &iris,
                di: None,
            };
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
        sodiumoxide::init().unwrap();
        let (public, secret) = box_::gen_keypair();
        let output = pcp::build(&request(&public.0), &mut rand::rngs::OsRng, |_| {
            Ok::<_, SignerError>(Vec::new())
        })
        .unwrap();
        for encrypted in [&output.tier0, &output.tier1, &output.tier2] {
            let gzip =
                sodiumoxide::crypto::sealedbox::open(encrypted, &public, &secret)
                    .expect("normal output must still be encrypted");
            tier(&gzip);
        }
    }
}
