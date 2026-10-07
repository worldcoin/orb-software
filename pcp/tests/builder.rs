use alkali::asymmetric::seal::curve25519xsalsa20poly1305 as sealedbox;
use orb_pcp::{
    self as pcp, v1, BackendKey, BackendKeys, BiometricPolicy, BuildError,
    BuildRequest, MetadataError,
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

fn capture_info(device_public_key: Option<&str>) -> v1::Info {
    let some = |value: &str| Some(value.to_owned());
    v1::Info {
        signup_id: some("synthetic"),
        signup_reason: some("test"),
        orb_id: some("orb"),
        operator_id: some("operator"),
        timestamp: some("0"),
        qr_code: some("qr"),
        id_commitment: some("id"),
        software_version: some("test"),
        orb_country: some("country"),
        orb_public_key_certificate: some("c3ludGhldGljLWNlcnRpZmljYXRl"),
        left_ir_image_id: some("left"),
        right_ir_image_id: some("right"),
        thumbnail_image_id: some("thumbnail"),
        device_public_key: device_public_key.map(str::to_owned),
        ..Default::default()
    }
}

fn request<'a>(key: &'a [u8; 32], info: &'a v1::Info) -> BuildRequest<'a> {
    let backend = || BackendKey {
        public_key: key,
        encrypted_private_key: "synthetic-envelope",
    };
    BuildRequest {
        timestamp: 1,
        info,
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

fn tier0(output: &pcp::Package, pair: &sealedbox::Keypair) -> Vec<(String, Vec<u8>)> {
    use std::io::Read;

    let gzip = open(&output.tier0, pair);
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(&gzip[..]));
    let mut files = Vec::new();
    for entry in archive.entries().unwrap() {
        let mut entry = entry.unwrap();
        let name = entry.path().unwrap().to_str().unwrap().to_owned();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        files.push((name, bytes));
    }
    files
}

fn normalized() -> pcp::NormalizedIrisFrame<'static> {
    pcp::NormalizedIrisFrame {
        image: b"image",
        mask: b"mask",
        image_resized: b"resized-image",
        mask_resized: b"resized-mask",
    }
}

fn images() -> pcp::PackageImages<'static> {
    let eye = || pcp::IrisEye {
        primary: pcp::PrimaryIrisFrame {
            ir_png: b"synthetic-ir",
            normalized: normalized(),
        },
        multiframe: &[],
    };
    pcp::PackageImages {
        left: eye(),
        right: eye(),
        thumbnail_png: Some(b"synthetic-thumbnail"),
        face_ir_png: None,
        thermal_png: None,
        fraud: None,
    }
}

fn iris_code_shares() -> [v1::IrisCodeShares; 3] {
    [0, 1, 2].map(|i| v1::IrisCodeShares {
        iris_version: Some("synthetic-iris".into()),
        iris_shares_version: Some("synthetic-sharing".into()),
        left_iris_code_shares: Some(format!("l{i}")),
        left_mask_code_shares: Some(format!("lm{i}")),
        right_iris_code_shares: Some(format!("r{i}")),
        right_mask_code_shares: Some(format!("rm{i}")),
    })
}

fn no_di() -> (v1::DiIrisEmbeddings, [v1::DiIrisEmbeddingShares; 3]) {
    Default::default()
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
fn device_key_is_optional_and_tiers_1_and_2_are_empty() {
    use std::io::Read;

    let pair = sealedbox::Keypair::generate().unwrap();
    for device in [None, Some("device"), Some("")] {
        let info = capture_info(device);
        let output = pcp::build(
            &request(&pair.public_key, &info),
            &mut rand::rngs::OsRng,
            |_| Ok::<_, SignerError>(b"synthetic-signature".to_vec()),
        )
        .unwrap();
        let files: std::collections::BTreeMap<_, _> =
            tier0(&output, &pair).into_iter().collect();
        assert_eq!(files.len(), 4);
        let json = |name: &str| -> serde_json::Value {
            serde_json::from_slice(&files[name]).unwrap()
        };
        let info = json("info.json");
        let manifest = json("hashes.json");
        assert_eq!(manifest["version"], "2.8");
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
        assert!(manifest
            .as_object()
            .unwrap()
            .keys()
            .all(|name| !name.starts_with("tier_")));
        for (tier, name) in [
            (&output.tier1, "tier1.tar.gz"),
            (&output.tier2, "tier2.tar.gz"),
        ] {
            let gzip = open(tier, &pair);
            let mut decoder = flate2::read::GzDecoder::new(gzip.as_slice());
            assert_eq!(decoder.header().unwrap().filename(), Some(name.as_bytes()));
            let mut tar = Vec::new();
            decoder.read_to_end(&mut tar).unwrap();
            assert_eq!(tar, [0; 1024]);
        }
    }
}

#[test]
fn invalid_user_key_fails_before_metadata_randomness() {
    let info = capture_info(Some("device"));
    let result = pcp::build(
        &request(&[0; 32], &info),
        &mut FailingRng,
        |_| -> Result<Vec<u8>, SignerError> { panic!("must not sign") },
    );
    assert!(matches!(
        result,
        Err(BuildError::Encryption(pcp::SealingError::InvalidRecipient))
    ));
}

#[test]
fn oversized_archive_timestamp_fails_before_signing() {
    let info = capture_info(Some("device"));
    let mut input = request(&[0; 32], &info);
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
    let info = capture_info(Some("device"));
    let result = pcp::build(
        &request(&pair.public_key, &info),
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
    let info = capture_info(Some("device"));
    let result = pcp::build(
        &request(&[0; 32], &info),
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
    let info = capture_info(Some("device"));
    let mut calls = 0;
    let result = pcp::build(
        &request(&pair.public_key, &info),
        &mut rand::rngs::OsRng,
        |_| {
            calls += 1;
            Err::<Vec<u8>, _>(SignerError)
        },
    );
    assert_eq!(calls, 1);
    assert!(matches!(
        result,
        Err(BuildError::Signing(pcp::SigningError::Signer(SignerError)))
    ));
}

#[test]
fn migration_pb_follows_info_and_is_hashed() {
    use orb_pcp_defs::prost::Message;

    let pair = sealedbox::Keypair::generate().unwrap();
    let info = capture_info(Some("device"));
    let migration = v1::Migration {
        tee_software_version: Some("synthetic-tee".into()),
        src_signup_id: Some("synthetic-source".into()),
        source_pcp_version: Some("2.6".into()),
        migrated_ts: Some(1_800_000_000),
        biometric_pipeline_version: Some("synthetic-pipeline".into()),
    };
    let mut input = request(&pair.public_key, &info);
    input.migration = Some(&migration);
    let output = pcp::build(&input, &mut rand::rngs::OsRng, |_| {
        Ok::<_, SignerError>(b"synthetic-signature".to_vec())
    })
    .unwrap();
    let files = tier0(&output, &pair);
    let names: Vec<_> = files.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        names,
        [
            "info.json",
            "migration.pb",
            "hashes.sign",
            "hashes.json",
            "backend_keys.json"
        ]
    );
    let files: std::collections::BTreeMap<_, _> = files.into_iter().collect();
    assert_eq!(files["migration.pb"], migration.encode_to_vec());
    let hashes: v1::Hashes = serde_json::from_slice(&files["hashes.json"]).unwrap();
    assert_eq!(hashes.version.as_deref(), Some("2.8"));
    let digest = ring::digest::digest(&ring::digest::SHA256, &files["migration.pb"]);
    assert_eq!(
        hashes.migration_pb,
        Some(data_encoding::HEXLOWER.encode(digest.as_ref()))
    );
}

/// Every tier 0 JSON file decodes into the shared `pcp-defs` types without
/// losing a key or value. Only per-frame manifest keys fall outside `Hashes`.
#[test]
fn tier0_json_matches_the_shared_pcp_defs_schema() {
    use std::collections::BTreeSet;

    macro_rules! assert_schema {
        ($ty:ty, $bytes:expr) => {{
            let value: serde_json::Value = serde_json::from_slice($bytes).unwrap();
            let typed: $ty = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(serde_json::to_value(typed).unwrap(), value);
        }};
    }

    let pair = sealedbox::Keypair::generate().unwrap();
    let extra_id: pcp::ImageId = "00ff0000000000000000000000000001".parse().unwrap();
    let extra = [pcp::IrisFrame {
        image_id: &extra_id,
        ir_png: b"synthetic-ir",
        normalized: Some(normalized()),
    }];
    let mut images = images();
    images.left.multiframe = &extra;
    let iris_codes = v1::IrisCodes {
        iris_version: Some("synthetic-iris".into()),
        left_iris_code: Some("left".into()),
        left_mask_code: Some("left".into()),
        right_iris_code: Some("right".into()),
        right_mask_code: Some("right".into()),
    };
    let iris_code_shares = iris_code_shares();
    let face_embeddings = [v1::FaceEmbedding {
        embedding: Some("synthetic-embedding".into()),
        embedding_type: Some("synthetic".into()),
        embedding_version: Some("v1".into()),
        embedding_inference_backend: Some("none".into()),
    }];
    let (di_embeddings, di_embedding_shares) = no_di();
    let mut info = capture_info(Some("device"));
    info.left_iris_code_aggregate_image_ids = vec!["left".into()];
    let mut input = request(&pair.public_key, &info);
    input.biometrics = BiometricPolicy::Included {
        images: &images,
        face_embeddings: &face_embeddings,
        iris_codes: &iris_codes,
        iris_code_shares: &iris_code_shares,
        di_embeddings: &di_embeddings,
        di_embedding_shares: &di_embedding_shares,
    };
    let output = pcp::build(&input, &mut rand::rngs::OsRng, |_| {
        Ok::<_, SignerError>(b"synthetic-signature".to_vec())
    })
    .unwrap();
    let files: std::collections::BTreeMap<_, _> =
        tier0(&output, &pair).into_iter().collect();

    assert_schema!(v1::Info, &files["info.json"]);
    assert_schema!(v1::BackendKeys, &files["backend_keys.json"]);
    assert_schema!(Vec<v1::FaceEmbedding>, &files["face_embeddings.json"]);
    assert_schema!(v1::IrisCodes, &files["iris_codes.json"]);
    for i in 0..3 {
        assert_schema!(
            v1::IrisCodeShares,
            &files[&format!("iris_code_shares_{i}.json")]
        );
    }

    let manifest: serde_json::Value =
        serde_json::from_slice(&files["hashes.json"]).unwrap();
    let known: v1::Hashes = serde_json::from_value(manifest.clone()).unwrap();
    let known = serde_json::to_value(known).unwrap();
    let (manifest, known) = (manifest.as_object().unwrap(), known.as_object().unwrap());
    for (name, hash) in known {
        assert_eq!(&manifest[name], hash, "{name}");
    }
    let unknown: BTreeSet<_> = manifest
        .keys()
        .filter(|name| !known.contains_key(*name))
        .cloned()
        .collect();
    let mut per_frame = BTreeSet::from([format!("{extra_id}.png")]);
    for kind in ["image", "mask"] {
        for part in ["", "_commitment", "_blinding_factors"] {
            for resized in ["", "_resized"] {
                per_frame
                    .insert(format!("{extra_id}_normalized_{kind}{part}{resized}.bin"));
            }
        }
    }
    assert_eq!(unknown, per_frame);
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
    fn diagnostic_redacted_tiers_are_gzip() {
        for device in [None, Some("device")] {
            let info = capture_info(device);
            let input = request(&[0; 32], &info);
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
            assert_eq!(manifest["version"], "2.8");
            assert_eq!(
                manifest["backend_keys.json"],
                hash(&tier0["backend_keys.json"])
            );
        }
    }

    #[test]
    fn diagnostic_included_inner_archives_are_plaintext() {
        let mut images = images();
        images.face_ir_png = Some(b"synthetic-face-ir");
        images.thermal_png = Some(b"synthetic-thermal");
        images.fraud = Some(pcp::FraudImages {
            scc_rgb_png: b"synthetic-scc",
            left_rgb_png: b"synthetic-left",
            right_rgb_png: b"synthetic-right",
            left_thermal_png: None,
            right_thermal_png: None,
            scc_depth_png: None,
            left_depth_png: None,
            right_depth_png: None,
        });
        let iris_codes = v1::IrisCodes::default();
        let iris_code_shares = iris_code_shares();
        let (di_embeddings, di_embedding_shares) = no_di();
        for device in [None, Some("device")] {
            let info = capture_info(device);
            let mut input = request(&[0; 32], &info);
            input.biometrics = BiometricPolicy::Included {
                images: &images,
                face_embeddings: &[],
                iris_codes: &iris_codes,
                iris_code_shares: &iris_code_shares,
                di_embeddings: &di_embeddings,
                di_embedding_shares: &di_embedding_shares,
            };
            let output = pcp::build_unencrypted_for_diagnostics(
                &input,
                &mut rand::rngs::OsRng,
                |_| Ok::<_, SignerError>(Vec::new()),
            )
            .unwrap();
            let archives = tier(&output.tier0);
            assert!(tier(&output.tier1).is_empty());
            assert!(tier(&output.tier2).is_empty());
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
            let modalities = entries(&archives["face_ir_and_thermal.tar"]);
            assert_eq!(modalities["face_ir.png"], b"synthetic-face-ir");
            assert_eq!(modalities["thermal.png"], b"synthetic-thermal");
        }
    }

    #[test]
    fn normal_builder_remains_encrypted_with_diagnostics_enabled() {
        let pair = sealedbox::Keypair::generate().unwrap();
        let info = capture_info(Some("device"));
        let output = pcp::build(
            &request(&pair.public_key, &info),
            &mut rand::rngs::OsRng,
            |_| Ok::<_, SignerError>(Vec::new()),
        )
        .unwrap();
        for encrypted in [&output.tier0, &output.tier1, &output.tier2] {
            let gzip = open(encrypted, &pair);
            tier(&gzip);
        }
    }
}
