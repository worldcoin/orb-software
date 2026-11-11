mod support;

use alkali::asymmetric::seal::curve25519xsalsa20poly1305 as sealedbox;
use orb_pcp::{
    self as pcp, v1, BackendKey, BackendKeys, BiometricPolicy, Biometrics, BuildError,
    Builder, Di, Iris, MetadataError,
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

fn backend_keys(key: &[u8; 32]) -> BackendKeys {
    let backend = || BackendKey {
        public_key: *key,
        encrypted_private_key: "synthetic-envelope".to_owned(),
    };
    BackendKeys::face(backend())
        .iris(backend())
        .normalized_iris(backend())
        .tier2(backend())
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

fn normalized() -> pcp::NormalizedIrisFrame {
    pcp::NormalizedIrisFrame {
        image: b"image".to_vec(),
        mask: b"mask".to_vec(),
        image_resized: b"resized-image".to_vec(),
        mask_resized: b"resized-mask".to_vec(),
    }
}

fn images() -> pcp::PackageImages {
    let eye = || pcp::IrisEye {
        primary: pcp::PrimaryIrisFrame {
            ir_png: b"synthetic-ir".to_vec(),
            normalized: normalized(),
        },
        multiframe: Vec::new(),
    };
    pcp::PackageImages {
        left: eye(),
        right: eye(),
        thumbnail_png: Some(b"synthetic-thumbnail".to_vec()),
        face_ir_png: None,
        thermal_png: None,
        fraud: None,
    }
}

fn iris_codes() -> v1::IrisCodes {
    v1::IrisCodes {
        iris_version: Some("synthetic-iris".into()),
        left_iris_code: Some("left".into()),
        left_mask_code: Some("left".into()),
        right_iris_code: Some("right".into()),
        right_mask_code: Some("right".into()),
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

fn di_biometrics() -> (v1::DiIrisEmbeddings, [v1::DiIrisEmbeddingShares; 3]) {
    let embeddings = v1::DiIrisEmbeddings {
        embedding_v1: Some(v1::DiIrisEmbeddingV1 {
            model_version: "synthetic-model".into(),
            embedding_inference_backend: "synthetic-backend".into(),
            embedding_version: "synthetic-embedding".into(),
            left_embedding: vec![-1, 2],
            left_mirror_embedding: vec![3, -4],
            right_embedding: vec![5, -6],
            right_mirror_embedding: vec![-7, 8],
            left_embedding_f32: vec![-1.0, 2.0],
            left_mirror_embedding_f32: vec![3.0, -4.0],
            right_embedding_f32: vec![5.0, -6.0],
            right_mirror_embedding_f32: vec![-7.0, 8.0],
        }),
    };
    let shares = [0, 1, 2].map(|i| v1::DiIrisEmbeddingShares {
        share_v1: Some(v1::DiIrisEmbeddingShareV1 {
            model_version: "synthetic-model".into(),
            shares_version: "synthetic-sharing".into(),
            embedding_version: "synthetic-embedding".into(),
            left_share: vec![10 + i, 11 + i],
            left_mirror_share: vec![20 + i, 21 + i],
            right_share: vec![30 + i, 31 + i],
            right_mirror_share: vec![40 + i, 41 + i],
        }),
    });
    (embeddings, shares)
}

fn di_embedding_5_party_shares() -> [v1::DiIrisEmbeddingShares; 5] {
    [0, 1, 2, 3, 4].map(|i| v1::DiIrisEmbeddingShares {
        share_v1: Some(v1::DiIrisEmbeddingShareV1 {
            model_version: "synthetic-model".into(),
            shares_version: "synthetic-5-party-sharing".into(),
            embedding_version: "synthetic-embedding".into(),
            left_share: vec![i; 512],
            left_mirror_share: vec![10 + i; 512],
            right_share: vec![20 + i; 512],
            right_mirror_share: vec![u32::from(u16::MAX) - i; 512],
        }),
    })
}

fn assert_iris_validation(codes: &v1::IrisCodes, shares: &[v1::IrisCodeShares; 3]) {
    assert!(Iris::codes(Some(codes.clone()))
        .shares(shares.clone().map(Some))
        .is_err());
}

#[test]
fn iris_constructor_rejects_missing_fields() {
    for field in 0..4 {
        // Arrange
        let mut codes = iris_codes();
        let shares = iris_code_shares();
        *[
            &mut codes.left_iris_code,
            &mut codes.left_mask_code,
            &mut codes.right_iris_code,
            &mut codes.right_mask_code,
        ][field] = None;

        // Act / Assert
        assert_iris_validation(&codes, &shares);
    }

    for recipient in 0..3 {
        for field in 0..4 {
            // Arrange
            let codes = iris_codes();
            let mut shares = iris_code_shares();
            let share = &mut shares[recipient];
            *[
                &mut share.left_iris_code_shares,
                &mut share.left_mask_code_shares,
                &mut share.right_iris_code_shares,
                &mut share.right_mask_code_shares,
            ][field] = None;

            // Act / Assert
            assert_iris_validation(&codes, &shares);
        }
    }
}

#[test]
fn di_rejects_missing_embedding_or_share_records() {
    // Arrange
    let (mut embeddings, shares) = di_biometrics();
    embeddings.embedding_v1 = None;

    // Act
    let result = Di::embeddings(embeddings).shares(shares);

    // Assert
    assert!(result.is_err(), "accepted missing embedding record");

    for recipient in 0..3 {
        // Arrange
        let (embeddings, mut shares) = di_biometrics();
        shares[recipient].share_v1 = None;

        // Act
        let result = Di::embeddings(embeddings).shares(shares);

        // Assert
        assert!(
            result.is_err(),
            "accepted missing record in share {recipient}"
        );
    }
}

#[test]
fn iris_constructor_rejects_mismatched_versions() {
    for recipient in 0..3 {
        // Arrange
        let codes = iris_codes();
        let mut shares = iris_code_shares();
        shares[recipient].iris_version = Some("different-iris".into());

        // Act / Assert
        assert_iris_validation(&codes, &shares);
    }
}

#[test]
fn di_rejects_mismatched_model_versions() {
    for recipient in 0..3 {
        // Arrange
        let (embeddings, mut shares) = di_biometrics();
        shares[recipient].share_v1.as_mut().unwrap().model_version =
            "different-model".into();

        // Act
        let result = Di::embeddings(embeddings).shares(shares);

        // Assert
        assert!(result.is_err(), "share {recipient} should be rejected");
    }
}

#[test]
fn di_rejects_mismatched_embedding_versions() {
    for recipient in 0..3 {
        // Arrange
        let (embeddings, mut shares) = di_biometrics();
        shares[recipient]
            .share_v1
            .as_mut()
            .unwrap()
            .embedding_version = "different-embedding".into();

        // Act
        let result = Di::embeddings(embeddings).shares(shares);

        // Assert
        assert!(result.is_err(), "share {recipient} should be rejected");
    }
}

#[test]
fn di_rejects_missing_five_party_share_records() {
    for recipient in 0..5 {
        // Arrange
        let (embeddings, shares) = di_biometrics();
        let mut five_party_shares = di_embedding_5_party_shares();
        five_party_shares[recipient].share_v1 = None;

        // Act
        let result = Di::embeddings(embeddings)
            .di_embedding_5_party_shares(five_party_shares)
            .shares(shares);

        // Assert
        assert!(
            result.is_err(),
            "accepted missing record in five-party share {recipient}"
        );
    }
}

#[test]
fn di_rejects_mismatched_five_party_share_versions() {
    let mismatches: [fn(&mut v1::DiIrisEmbeddingShareV1); 2] = [
        |share| share.model_version = "different-model".into(),
        |share| share.embedding_version = "different-embedding".into(),
    ];
    for mismatch in mismatches {
        for recipient in 0..5 {
            // Arrange
            let (embeddings, shares) = di_biometrics();
            let mut five_party_shares = di_embedding_5_party_shares();
            mismatch(five_party_shares[recipient].share_v1.as_mut().unwrap());

            // Act
            let result = Di::embeddings(embeddings)
                .di_embedding_5_party_shares(five_party_shares)
                .shares(shares);

            // Assert
            assert!(
                result.is_err(),
                "five-party share {recipient} should be rejected"
            );
        }
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
fn device_key_is_optional_and_tiers_1_and_2_are_empty() {
    use std::io::Read;

    let pair = sealedbox::Keypair::generate().unwrap();
    for device in [None, Some("device"), Some("")] {
        let info = capture_info(device);
        let output = Builder::timestamp(1)
            .info(info)
            .user_public_key(pair.public_key)
            .backend_keys(backend_keys(&pair.public_key))
            .biometrics(BiometricPolicy::Redacted)
            .rng(rand::rngs::OsRng)
            .sealer(support::seal)
            .signer(|_| Ok::<_, SignerError>(b"synthetic-signature".to_vec()))
            .build()
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
    let result = Builder::timestamp(1)
        .info(info)
        .user_public_key([0; 32])
        .backend_keys(backend_keys(&[0; 32]))
        .biometrics(BiometricPolicy::Redacted)
        .rng(FailingRng)
        .sealer(support::seal)
        .signer(|_| -> Result<Vec<u8>, SignerError> { panic!("must not sign") })
        .build();
    assert!(matches!(
        result,
        Err(BuildError::Encryption(
            support::SealingError::InvalidRecipient
        ))
    ));
}

#[test]
fn randomness_failure_returns_no_package_or_signature() {
    let pair = sealedbox::Keypair::generate().unwrap();
    let info = capture_info(Some("device"));
    let result = Builder::timestamp(1)
        .info(info)
        .user_public_key(pair.public_key)
        .backend_keys(backend_keys(&pair.public_key))
        .biometrics(BiometricPolicy::Redacted)
        .rng(FailingRng)
        .sealer(support::seal)
        .signer(|_| -> Result<Vec<u8>, SignerError> { panic!("must not sign") })
        .build();
    assert!(matches!(
        result,
        Err(BuildError::Metadata(MetadataError::Randomness(_)))
    ));
}

#[test]
fn signer_failure_is_not_retried_or_returned_as_a_package() {
    let pair = sealedbox::Keypair::generate().unwrap();
    let info = capture_info(Some("device"));
    let mut calls = 0;
    let result = Builder::timestamp(1)
        .info(info)
        .user_public_key(pair.public_key)
        .backend_keys(backend_keys(&pair.public_key))
        .biometrics(BiometricPolicy::Redacted)
        .rng(rand::rngs::OsRng)
        .sealer(support::seal)
        .signer(|_| {
            calls += 1;
            Err::<Vec<u8>, _>(SignerError)
        })
        .build();
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
        signing_key_id: Some("synthetic-signing-key".into()),
    };
    let output = Builder::timestamp(1)
        .info(info)
        .user_public_key(pair.public_key)
        .backend_keys(backend_keys(&pair.public_key))
        .biometrics(BiometricPolicy::Redacted)
        .migration(migration.clone())
        .rng(rand::rngs::OsRng)
        .sealer(support::seal)
        .signer(|_| Ok::<_, SignerError>(b"synthetic-signature".to_vec()))
        .build()
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

#[test]
fn iris_and_di_groups_are_included_or_omitted_independently() {
    use orb_pcp_defs::prost::Message;

    for (iris_present, di_present) in
        [(false, false), (true, false), (false, true), (true, true)]
    {
        // Arrange
        let pair = sealedbox::Keypair::generate().unwrap();
        let info = capture_info(None);
        let images = images();
        let codes = iris_codes();
        let iris_shares = iris_code_shares();
        let (embeddings, di_shares) = di_biometrics();
        let iris = iris_present.then(|| {
            Iris::codes(Some(codes.clone()))
                .shares(iris_shares.clone().map(Some))
                .unwrap()
        });
        let di = di_present.then(|| {
            Di::embeddings(embeddings.clone())
                .shares(di_shares.clone())
                .unwrap()
        });
        let biometrics = Biometrics::images(images)
            .maybe_iris(iris)
            .maybe_di(di)
            .face_embeddings(Vec::new());

        // Act
        let output = Builder::timestamp(1)
            .info(info)
            .user_public_key(pair.public_key)
            .backend_keys(backend_keys(&pair.public_key))
            .biometrics(BiometricPolicy::Included(biometrics))
            .rng(rand::rngs::OsRng)
            .sealer(support::seal)
            .signer(|_| Ok::<_, SignerError>(b"synthetic-signature".to_vec()))
            .build()
            .unwrap();

        // Assert
        let files: std::collections::BTreeMap<_, _> =
            tier0(&output, &pair).into_iter().collect();
        let manifest: serde_json::Value =
            serde_json::from_slice(&files["hashes.json"]).unwrap();
        for (names, present) in [
            (
                [
                    "iris_codes.json",
                    "iris_code_shares_0.json",
                    "iris_code_shares_1.json",
                    "iris_code_shares_2.json",
                ],
                iris_present,
            ),
            (
                [
                    "di_iris_embeddings.pb",
                    "di_iris_embeddings_shares_0.pb",
                    "di_iris_embeddings_shares_1.pb",
                    "di_iris_embeddings_shares_2.pb",
                ],
                di_present,
            ),
        ] {
            for name in names {
                assert_eq!(files.contains_key(name), present, "{name}");
                let expected_hash = files.get(name).map(|bytes| {
                    data_encoding::HEXLOWER.encode(
                        ring::digest::digest(&ring::digest::SHA256, bytes).as_ref(),
                    )
                });
                assert_eq!(
                    manifest.get(name).and_then(serde_json::Value::as_str),
                    expected_hash.as_deref(),
                    "{name}"
                );
            }
        }
        if iris_present {
            assert_eq!(
                serde_json::from_slice::<v1::IrisCodes>(&files["iris_codes.json"])
                    .unwrap(),
                codes
            );
            for (i, share) in iris_shares.iter().enumerate() {
                let decoded: v1::IrisCodeShares = serde_json::from_slice(
                    &files[&format!("iris_code_shares_{i}.json")],
                )
                .unwrap();
                assert_eq!(&decoded, share);
            }
        }
        if di_present {
            assert_eq!(
                v1::DiIrisEmbeddings::decode(files["di_iris_embeddings.pb"].as_slice())
                    .unwrap(),
                embeddings
            );
            for (i, share) in di_shares.iter().enumerate() {
                let decoded = v1::DiIrisEmbeddingShares::decode(
                    files[&format!("di_iris_embeddings_shares_{i}.pb")].as_slice(),
                )
                .unwrap();
                assert_eq!(&decoded, share);
            }
        }
    }
}

#[test]
fn five_party_presence_and_redaction_apply_to_all_five_files_and_hashes() {
    use orb_pcp_defs::prost::Message;

    let pair = sealedbox::Keypair::generate().unwrap();
    let info = capture_info(None);
    let (embeddings, di_shares) = di_biometrics();
    let shares = di_embedding_5_party_shares();
    for (included, present) in [(true, false), (true, true), (false, true)] {
        let di = Di::embeddings(embeddings.clone())
            .maybe_di_embedding_5_party_shares(present.then(|| shares.clone()))
            .shares(di_shares.clone())
            .unwrap();
        let biometrics = if included {
            BiometricPolicy::Included(
                Biometrics::images(images())
                    .di(di)
                    .face_embeddings(Vec::new()),
            )
        } else {
            BiometricPolicy::Redacted
        };
        let output = Builder::timestamp(1)
            .info(info.clone())
            .user_public_key(pair.public_key)
            .backend_keys(backend_keys(&pair.public_key))
            .biometrics(biometrics)
            .rng(rand::rngs::OsRng)
            .sealer(support::seal)
            .signer(|_| Ok::<_, SignerError>(Vec::new()))
            .build()
            .unwrap();
        let files: std::collections::BTreeMap<_, _> =
            tier0(&output, &pair).into_iter().collect();
        let manifest: serde_json::Value =
            serde_json::from_slice(&files["hashes.json"]).unwrap();
        for (i, share) in shares.iter().enumerate() {
            let name = format!("di_iris_embeddings_5_party_shares_{i}.pb");
            let expected = included && present;
            assert_eq!(files.contains_key(&name), expected, "{name}");
            assert_eq!(manifest.get(&name).is_some(), expected, "{name}");
            if expected {
                assert_eq!(files[&name], share.encode_to_vec(), "{name}");
                assert_eq!(
                    manifest[&name],
                    data_encoding::HEXLOWER.encode(
                        ring::digest::digest(&ring::digest::SHA256, &files[&name])
                            .as_ref()
                    )
                );
            }
        }
    }
}

/// Every tier 0 JSON file decodes into the shared `pcp-defs` types without
/// losing a key or value. Only per-frame manifest keys fall outside `Hashes`.
#[test]
fn tier0_json_matches_the_shared_pcp_defs_schema() {
    use orb_pcp_defs::prost::Message;
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
    let extra = vec![pcp::IrisFrame {
        image_id: extra_id.clone(),
        ir_png: b"synthetic-ir".to_vec(),
        normalized: Some(normalized()),
    }];
    let mut images = images();
    images.left.multiframe = extra;
    let iris_codes = iris_codes();
    let iris_code_shares = iris_code_shares();
    let face_embeddings = [v1::FaceEmbedding {
        embedding: Some("synthetic-embedding".into()),
        embedding_type: Some("synthetic".into()),
        embedding_version: Some("v1".into()),
        embedding_inference_backend: Some("none".into()),
    }];
    let (di_embeddings, di_embedding_shares) = di_biometrics();
    let di_embedding_5_party_shares = di_embedding_5_party_shares();
    let mut info = capture_info(Some("device"));
    info.left_iris_code_aggregate_image_ids = vec!["left".into()];
    let iris = Iris::codes(Some(iris_codes))
        .shares(iris_code_shares.map(Some))
        .unwrap();
    let di = Di::embeddings(di_embeddings)
        .di_embedding_5_party_shares(di_embedding_5_party_shares.clone())
        .shares(di_embedding_shares)
        .unwrap();
    let biometrics = Biometrics::images(images)
        .iris(iris)
        .di(di)
        .face_embeddings(face_embeddings.into());
    let output = Builder::timestamp(1)
        .info(info)
        .user_public_key(pair.public_key)
        .backend_keys(backend_keys(&pair.public_key))
        .biometrics(BiometricPolicy::Included(biometrics))
        .rng(rand::rngs::OsRng)
        .sealer(support::seal)
        .signer(|_| Ok::<_, SignerError>(b"synthetic-signature".to_vec()))
        .build()
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

    for (i, expected) in di_embedding_5_party_shares.iter().enumerate() {
        let name = format!("di_iris_embeddings_5_party_shares_{i}.pb");
        assert_eq!(files[&name], expected.encode_to_vec());
        assert_eq!(
            v1::DiIrisEmbeddingShares::decode(files[&name].as_slice()).unwrap(),
            *expected
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
            let mut calls = 0;
            let output: pcp::DiagnosticPackage = Builder::timestamp(1)
                .info(info)
                .user_public_key([0; 32])
                .backend_keys(backend_keys(&[0; 32]))
                .biometrics(BiometricPolicy::Redacted)
                .rng(rand::rngs::OsRng)
                .sealer(|_, _| -> Result<Vec<u8>, std::convert::Infallible> {
                    panic!("diagnostics must not invoke the sealer")
                })
                .signer(|_| {
                    calls += 1;
                    Ok::<_, SignerError>(b"synthetic-signature".to_vec())
                })
                .build_unencrypted()
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
        for device in [None, Some("device")] {
            let mut images = images();
            images.face_ir_png = Some(b"synthetic-face-ir".to_vec());
            images.thermal_png = Some(b"synthetic-thermal".to_vec());
            images.fraud = Some(pcp::FraudImages {
                scc_rgb_png: Some(b"synthetic-scc".to_vec()),
                left_rgb_png: Some(b"synthetic-left".to_vec()),
                right_rgb_png: Some(b"synthetic-right".to_vec()),
                left_thermal_png: None,
                right_thermal_png: None,
                scc_depth_png: None,
                left_depth_png: None,
                right_depth_png: None,
            });
            let iris_codes = iris_codes();
            let iris_code_shares = iris_code_shares();
            let (di_embeddings, di_embedding_shares) = di_biometrics();
            let info = capture_info(device);
            let iris = Iris::codes(Some(iris_codes))
                .shares(iris_code_shares.map(Some))
                .unwrap();
            let di = Di::embeddings(di_embeddings)
                .shares(di_embedding_shares)
                .unwrap();
            let biometrics = Biometrics::images(images)
                .iris(iris)
                .di(di)
                .face_embeddings(Vec::new());
            let output = Builder::timestamp(1)
                .info(info)
                .user_public_key([0; 32])
                .backend_keys(backend_keys(&[0; 32]))
                .biometrics(BiometricPolicy::Included(biometrics))
                .rng(rand::rngs::OsRng)
                .sealer(|_, _| -> Result<Vec<u8>, std::convert::Infallible> {
                    panic!("diagnostics must not invoke the sealer")
                })
                .signer(|_| Ok::<_, SignerError>(Vec::new()))
                .build_unencrypted()
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
        let output = Builder::timestamp(1)
            .info(info)
            .user_public_key(pair.public_key)
            .backend_keys(backend_keys(&pair.public_key))
            .biometrics(BiometricPolicy::Redacted)
            .rng(rand::rngs::OsRng)
            .sealer(support::seal)
            .signer(|_| Ok::<_, SignerError>(Vec::new()))
            .build()
            .unwrap();
        for encrypted in [&output.tier0, &output.tier1, &output.tier2] {
            let gzip = open(encrypted, &pair);
            tier(&gzip);
        }
    }
}

#[test]
fn iris_constructor_validates_shares_without_codes() {
    // Arrange
    let shares = iris_code_shares();
    let mut invalid_shares = iris_code_shares();
    invalid_shares[1].left_iris_code_shares = None;

    // Act
    let valid = Iris::codes(None).shares(shares.map(Some));
    let invalid = Iris::codes(None).shares(invalid_shares.map(Some));
    let absent = Iris::codes(None).shares([None, None, None]);

    // Assert
    assert!(valid.is_ok());
    assert!(invalid.is_err());
    assert!(absent.is_ok());
}

#[test]
fn strict_mode_requires_iris_records_and_compatibility_mode_omits_missing_files() {
    // Arrange
    let pair = sealedbox::Keypair::generate().unwrap();
    let info = capture_info(None);
    let codes = iris_codes();
    let shares = iris_code_shares();

    for present in 0u8..16 {
        let codes = (present & 1 != 0).then_some(&codes);
        let shares =
            std::array::from_fn(|i| (present & (2 << i) != 0).then_some(&shares[i]));

        for strict in [None, Some(true), Some(false)] {
            // Arrange
            let iris = Iris::codes(codes.cloned())
                .shares(shares.map(|share| share.cloned()))
                .unwrap();
            let biometrics = Biometrics::images(images())
                .iris(iris)
                .face_embeddings(Vec::new());
            let mut signed = false;

            // Act
            let result = Builder::timestamp(1)
                .info(info.clone())
                .user_public_key(pair.public_key)
                .backend_keys(backend_keys(&pair.public_key))
                .biometrics(BiometricPolicy::Included(biometrics))
                .maybe_strict(strict)
                .rng(&mut rand::rngs::OsRng)
                .sealer(support::seal)
                .signer(|_| {
                    signed = true;

                    Ok::<_, SignerError>(vec![1])
                })
                .build();

            // Assert
            let complete = codes.is_some() && shares.iter().all(Option::is_some);

            if strict != Some(false) && !complete {
                assert!(matches!(result, Err(BuildError::InvalidBiometrics(_))));
                assert!(!signed);

                continue;
            }

            let package = result.unwrap();
            assert!(signed);
            let files: std::collections::BTreeMap<_, _> =
                tier0(&package, &pair).into_iter().collect();
            let manifest: serde_json::Value =
                serde_json::from_slice(&files["hashes.json"]).unwrap();

            let mut expected_files = vec![(
                "iris_codes.json".to_owned(),
                codes.map(|codes| serde_json::to_value(codes).unwrap()),
            )];

            for (i, share) in shares.iter().enumerate() {
                expected_files.push((
                    format!("iris_code_shares_{i}.json"),
                    share.map(|share| serde_json::to_value(share).unwrap()),
                ));
            }

            for (name, expected) in expected_files {
                assert_eq!(files.contains_key(&name), expected.is_some());
                assert_eq!(manifest.get(&name).is_some(), expected.is_some());

                if let Some(expected) = expected {
                    let actual: serde_json::Value =
                        serde_json::from_slice(&files[&name]).unwrap();
                    let hash =
                        ring::digest::digest(&ring::digest::SHA256, &files[&name]);

                    assert_eq!(actual, expected);
                    assert_eq!(
                        manifest[&name],
                        data_encoding::HEXLOWER.encode(hash.as_ref()),
                    );
                }
            }
        }
    }
}

#[test]
fn missing_iris_versions_require_compatibility_mode() {
    // Arrange
    let pair = sealedbox::Keypair::generate().unwrap();
    let info = capture_info(None);

    for missing in 0..8 {
        let mut codes = iris_codes();
        let mut shares = iris_code_shares();
        match missing {
            0 => codes.iris_version = None,
            1..=3 => shares[missing - 1].iris_version = None,
            4..=6 => shares[missing - 4].iris_shares_version = None,
            _ => {
                codes.iris_version = None;
                for share in &mut shares {
                    share.iris_version = None;
                    share.iris_shares_version = None;
                }
            }
        }

        for strict in [None, Some(true), Some(false)] {
            // Arrange
            let mut signed = false;

            // Act
            let iris = Iris::codes(Some(codes.clone()))
                .shares(shares.clone().map(Some))
                .expect("missing versions must be accepted by the constructor");
            let biometrics = Biometrics::images(images())
                .iris(iris)
                .face_embeddings(Vec::new());
            let result = Builder::timestamp(1)
                .info(info.clone())
                .user_public_key(pair.public_key)
                .backend_keys(backend_keys(&pair.public_key))
                .biometrics(BiometricPolicy::Included(biometrics))
                .maybe_strict(strict)
                .rng(&mut rand::rngs::OsRng)
                .sealer(support::seal)
                .signer(|_| {
                    signed = true;

                    Ok::<_, SignerError>(vec![1])
                })
                .build();

            // Assert
            if strict != Some(false) {
                assert!(matches!(result, Err(BuildError::InvalidBiometrics(_))));
                assert!(!signed);

                continue;
            }

            let package =
                result.expect("compatibility mode must accept missing versions");
            assert!(signed);
            let files: std::collections::BTreeMap<_, _> =
                tier0(&package, &pair).into_iter().collect();
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&files["iris_codes.json"])
                    .unwrap(),
                serde_json::to_value(&codes).unwrap(),
            );

            for (i, share) in shares.iter().enumerate() {
                assert_eq!(
                    serde_json::from_slice::<serde_json::Value>(
                        &files[&format!("iris_code_shares_{i}.json")]
                    )
                    .unwrap(),
                    serde_json::to_value(share).unwrap(),
                );
            }
        }
    }
}
