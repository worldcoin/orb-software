//! In-memory synthetic package construction and round-trip checks, not an
//! untrusted-package verifier or a production signer implementation.
//!
//! Run with `cargo run -p orb-pcp --example build_pcp`. Builds and verifies
//! PCP 2.8 packages with and without a device key, included and redacted.
//! Everything stays in memory; only case labels and encrypted sizes are printed.

use std::{collections::BTreeMap, error::Error, io::Read};

use alkali::asymmetric::seal::curve25519xsalsa20poly1305 as sealedbox;
use data_encoding::{BASE64, HEXLOWER};
use flate2::read::GzDecoder;
use orb_pcp::{self as pcp, v1};
use orb_pcp_defs::{
    prost::Message,
    v1::{DiIrisEmbeddingShares, DiIrisEmbeddings},
};
use p256::ecdsa::{
    signature::hazmat::{PrehashSigner, PrehashVerifier},
    Signature, SigningKey,
};
use rand::rngs::OsRng;
use ring::digest::{digest, SHA256};

type Files = BTreeMap<String, Vec<u8>>;
type KeyPair = sealedbox::Keypair;
type Result<T> = std::result::Result<T, Box<dyn Error>>;

const TIMESTAMP: u64 = 1_700_000_000;
// A synthetic 1x1 white grayscale/alpha PNG, never a captured image.
const PNG_BASE64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+ip1sAAAAASUVORK5CYII=";

fn main() -> Result<()> {
    run()
}

/// Also exercised by the integration test without filesystem or service access.
pub fn run() -> Result<()> {
    let png = BASE64.decode(PNG_BASE64.as_bytes())?;
    let normalized = [0x5a; 512];
    let extra_left = [pcp::IrisFrame {
        image_id: "synthetic-extra-left",
        ir_png: &png,
        normalized: None,
    }];
    let extra_right = [pcp::IrisFrame {
        image_id: "synthetic-extra-right",
        ir_png: &png,
        normalized: None,
    }];
    let images = pcp::PackageImages {
        left: Some(pcp::IrisEye {
            primary: primary(&png, &normalized),
            multiframe: &extra_left,
        }),
        right: Some(pcp::IrisEye {
            primary: primary(&png, &normalized),
            multiframe: &extra_right,
        }),
        thumbnail_png: Some(&png),
        face_ir_png: Some(&png),
        thermal_png: Some(&png),
        fraud: Some(pcp::FraudImages {
            scc_rgb_png: &png,
            left_rgb_png: &png,
            right_rgb_png: &png,
            left_thermal_png: Some(&png),
            right_thermal_png: Some(&png),
            scc_depth_png: Some(&png),
            left_depth_png: Some(&png),
            right_depth_png: Some(&png),
        }),
    };
    let iris_codes = v1::IrisCodes {
        iris_version: Some("synthetic".into()),
        left_iris_code: Some("synthetic-left-iris".into()),
        left_mask_code: Some("synthetic-left-mask".into()),
        right_iris_code: Some("synthetic-right-iris".into()),
        right_mask_code: Some("synthetic-right-mask".into()),
    };
    let iris_code_shares = [0, 1, 2].map(|i| v1::IrisCodeShares {
        iris_version: Some("synthetic".into()),
        iris_shares_version: Some("synthetic-sharing".into()),
        left_iris_code_shares: Some(format!("left-iris-{i}")),
        left_mask_code_shares: Some(format!("left-mask-{i}")),
        right_iris_code_shares: Some(format!("right-iris-{i}")),
        right_mask_code_shares: Some(format!("right-mask-{i}")),
    });
    let di_embeddings = v1::DiIrisEmbeddings {
        embedding_v1: Some(v1::DiIrisEmbeddingV1 {
            model_version: "synthetic-model".into(),
            embedding_inference_backend: "none".into(),
            embedding_version: "synthetic-embedding".into(),
            left_embedding: vec![-1, 2],
            left_mirror_embedding: vec![3, -4],
            right_embedding: vec![5, -6],
            right_mirror_embedding: vec![-7, 8],
            left_embedding_f32: vec![1.0, 2.0],
            left_mirror_embedding_f32: vec![3.0, -4.0],
            right_embedding_f32: vec![5.0, -6.0],
            right_mirror_embedding_f32: vec![-7.0, 8.0],
        }),
    };
    let di_embedding_shares = [0, 1, 2].map(|i| v1::DiIrisEmbeddingShares {
        share_v1: Some(v1::DiIrisEmbeddingShareV1 {
            model_version: "synthetic-model".into(),
            shares_version: "synthetic-di-sharing".into(),
            embedding_version: "synthetic-embedding".into(),
            left_share: vec![10 + i],
            left_mirror_share: vec![20 + i],
            right_share: vec![30 + i],
            right_mirror_share: vec![40 + i],
        }),
    });
    let face_embeddings = [v1::FaceEmbedding {
        embedding: Some("synthetic-embedding".into()),
        embedding_type: Some("synthetic".into()),
        embedding_version: Some("example".into()),
        embedding_inference_backend: Some("none".into()),
    }];

    // Callers supply their actual metadata; archive, encryption and manifest
    // choices belong to the builder.
    for device_public_key in [None, Some("synthetic-device-key")] {
        let some = |value: &str| Some(value.to_owned());
        let info = v1::Info {
            signup_id: some("synthetic-signup"),
            signup_reason: some("synthetic-example"),
            orb_id: some("synthetic-orb"),
            operator_id: some("synthetic-operator"),
            timestamp: Some(TIMESTAMP.to_string()),
            qr_code: some("synthetic-qr"),
            id_commitment: some("synthetic-id-commitment"),
            software_version: some("synthetic-example"),
            orb_country: some("XX"),
            orb_public_key_certificate: Some(
                BASE64.encode(b"synthetic-placeholder-not-a-certificate"),
            ),
            left_ir_image_id: some("synthetic-left"),
            right_ir_image_id: some("synthetic-right"),
            thumbnail_image_id: some("synthetic-thumbnail"),
            left_iris_code_aggregate_image_ids: vec!["synthetic-left".into()],
            right_iris_code_aggregate_image_ids: vec!["synthetic-right".into()],
            device_public_key: device_public_key.map(str::to_owned),
            ..Default::default()
        };
        for redacted in [false, true] {
            let user = KeyPair::generate()?;
            let backends = [
                KeyPair::generate()?,
                KeyPair::generate()?,
                KeyPair::generate()?,
                KeyPair::generate()?,
            ];
            let encrypted_keys: Vec<String> = backends
                .iter()
                .map(|pair| {
                    let mut ciphertext =
                        [0; sealedbox::PRIVATE_KEY_LENGTH + sealedbox::OVERHEAD_LENGTH];
                    sealedbox::encrypt(
                        pair.private_key.as_ref(),
                        &user.public_key,
                        &mut ciphertext,
                    )?;
                    Ok(BASE64.encode(&ciphertext))
                })
                .collect::<Result<_>>()?;
            let request = pcp::BuildRequest {
                timestamp: TIMESTAMP,
                info: &info,
                user_public_key: &user.public_key,
                backend_keys: pcp::BackendKeys {
                    iris: backend_key(&backends[0], &encrypted_keys[0]),
                    normalized_iris: backend_key(&backends[1], &encrypted_keys[1]),
                    face: backend_key(&backends[2], &encrypted_keys[2]),
                    tier2: backend_key(&backends[3], &encrypted_keys[3]),
                },
                biometrics: if redacted {
                    pcp::BiometricPolicy::Redacted
                } else {
                    pcp::BiometricPolicy::Included {
                        images: &images,
                        face_embeddings: &face_embeddings,
                        iris_codes: &iris_codes,
                        iris_code_shares: &iris_code_shares,
                        di_embeddings: &di_embeddings,
                        di_embedding_shares: &di_embedding_shares,
                    }
                },
                migration: None,
            };
            // P-256 is only this example's caller-owned signer choice.
            let signer = SigningKey::random(&mut OsRng);
            let mut calls = 0;
            let package = pcp::build(&request, &mut OsRng, |hash| {
                calls += 1;
                let signature: Signature = signer.sign_prehash(hash)?;
                Ok::<_, p256::ecdsa::Error>(signature.to_der().as_bytes().to_vec())
            })?;
            assert_eq!(calls, 1);
            verify(
                &package,
                device_public_key,
                redacted,
                &user,
                &backends,
                &signer,
            )?;
            println!(
                "device_key={} redacted={redacted}: encrypted tier lengths [{}, {}, {}]; verified",
                device_public_key.is_some(), package.tier0.len(), package.tier1.len(), package.tier2.len()
            );
        }
    }
    Ok(())
}

fn primary<'a>(png: &'a [u8], data: &'a [u8]) -> pcp::PrimaryIrisFrame<'a> {
    pcp::PrimaryIrisFrame {
        ir_png: png,
        normalized: pcp::NormalizedIrisFrame {
            image: data,
            mask: data,
            image_resized: data,
            mask_resized: data,
        },
    }
}

fn backend_key<'a>(pair: &'a KeyPair, encrypted: &'a str) -> pcp::BackendKey<'a> {
    pcp::BackendKey {
        public_key: &pair.public_key,
        encrypted_private_key: encrypted,
    }
}

fn hash(bytes: &[u8]) -> String {
    HEXLOWER.encode(digest(&SHA256, bytes).as_ref())
}

fn open(ciphertext: &[u8], pair: &KeyPair) -> Result<Vec<u8>> {
    let mut plaintext =
        vec![0; ciphertext.len().saturating_sub(sealedbox::OVERHEAD_LENGTH)];
    sealedbox::decrypt(ciphertext, pair, &mut plaintext)?;
    Ok(plaintext)
}

fn files(bytes: &[u8]) -> Result<Files> {
    let mut result = Files::new();
    for entry in tar::Archive::new(bytes).entries()? {
        let mut entry = entry?;
        assert_eq!(entry.header().mtime()?, TIMESTAMP);
        let name = entry
            .path()?
            .to_str()
            .ok_or("non-UTF-8 filename")?
            .to_owned();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        assert!(result.insert(name, bytes).is_none(), "duplicate tar entry");
    }
    Ok(result)
}

fn tier(bytes: &[u8], pair: &KeyPair, name: &str) -> Result<Files> {
    let plaintext = open(bytes, pair)?;
    let mut gzip = GzDecoder::new(plaintext.as_slice());
    let header = gzip.header().ok_or("missing gzip header")?;
    assert_eq!(header.mtime(), TIMESTAMP as u32);
    assert_eq!(
        header.filename(),
        Some(name.as_bytes()),
        "gzip filename mismatch"
    );
    let mut tar = Vec::new();
    gzip.read_to_end(&mut tar)?;
    files(&tar)
}

fn verify(
    package: &pcp::Package,
    device_public_key: Option<&str>,
    redacted: bool,
    user: &KeyPair,
    backends: &[KeyPair; 4],
    signer: &SigningKey,
) -> Result<()> {
    for (bytes, checksum) in [
        (&package.tier0, &package.tier0_checksum),
        (&package.tier1, &package.tier1_checksum),
        (&package.tier2, &package.tier2_checksum),
    ] {
        assert_eq!(
            digest(&SHA256, bytes).as_ref(),
            checksum,
            "checksum mismatch"
        );
    }
    let tier0 = tier(&package.tier0, user, "tier0.tar.gz")?;
    let tier1 = tier(&package.tier1, user, "tier1.tar.gz")?;
    let tier2 = tier(&package.tier2, user, "tier2.tar.gz")?;
    let manifest: BTreeMap<String, String> =
        serde_json::from_slice(&tier0["hashes.json"])?;
    let signature = Signature::from_der(&tier0["hashes.sign"])?;
    signer
        .verifying_key()
        .verify_prehash(digest(&SHA256, &tier0["hashes.json"]).as_ref(), &signature)?;
    let info: serde_json::Value = serde_json::from_slice(&tier0["info.json"])?;
    let mut expected = BTreeMap::new();
    for (name, value) in info.as_object().ok_or("metadata is not an object")? {
        if let Some(field) = name.strip_suffix("_salt") {
            let salt = value.as_str().ok_or("salt is not a string")?;
            assert_eq!(salt.len(), 32);
            assert_eq!(HEXLOWER.decode(salt.as_bytes())?.len(), 16);
            let value = info[field].as_str().ok_or("salted value is not a string")?;
            expected
                .insert(field.to_owned(), hash(format!("{value}{salt}").as_bytes()));
        }
    }
    assert_eq!(expected.len(), 9 + usize::from(device_public_key.is_some()));
    assert_eq!(
        expected.contains_key("device_public_key"),
        device_public_key.is_some()
    );
    for name in [
        "signup_id",
        "signup_reason",
        "orb_id",
        "operator_id",
        "timestamp",
        "qr_code",
        "id_commitment",
        "software_version",
        "orb_country",
    ] {
        assert!(
            expected.contains_key(name),
            "required metadata hash is missing"
        );
    }
    let keys: serde_json::Value = serde_json::from_slice(&tier0["backend_keys.json"])?;
    for (role, pair) in ["iris", "normalized_iris", "face", "tier2"]
        .into_iter()
        .zip(backends)
    {
        assert_eq!(
            keys[role]["public_key"],
            BASE64.encode(&pair.public_key),
            "backend public key mismatch"
        );
        let encrypted = keys[role]["encrypted_private_key"]
            .as_str()
            .ok_or("encrypted backend key is not a string")?;
        let recovered = open(&BASE64.decode(encrypted.as_bytes())?, user)?;
        assert_eq!(
            recovered,
            pair.private_key.as_ref(),
            "backend private key mismatch"
        );
    }
    expected.insert(
        "backend_keys.json".to_owned(),
        hash(&tier0["backend_keys.json"]),
    );
    expected.insert("version".to_owned(), "2.8".to_owned());
    assert!(tier1.is_empty() && tier2.is_empty());
    assert_eq!(
        info.get("device_public_key"),
        device_public_key.map(serde_json::Value::from).as_ref()
    );
    if redacted {
        assert_eq!(tier0.len(), 4);
        for name in [
            "left_ir_image_id",
            "right_ir_image_id",
            "thumbnail_image_id",
        ] {
            assert_eq!(info[name], "", "redacted image ID remains");
        }
        for name in [
            "left_ir_multiframe_image_ids",
            "right_ir_multiframe_image_ids",
            "left_iris_code_aggregate_image_ids",
            "right_iris_code_aggregate_image_ids",
        ] {
            assert!(
                info[name].as_array().is_some_and(Vec::is_empty),
                "redacted image IDs remain"
            );
        }
    } else {
        let iris: serde_json::Value =
            serde_json::from_slice(&tier0["iris_codes.json"])?;
        assert_eq!(iris["IRIS_version"], "synthetic");
        assert_eq!(iris["left_iris_code"], "synthetic-left-iris");
        assert_eq!(iris["left_mask_code"], "synthetic-left-mask");
        assert_eq!(iris["right_iris_code"], "synthetic-right-iris");
        assert_eq!(iris["right_mask_code"], "synthetic-right-mask");
        let di = DiIrisEmbeddings::decode(tier0["di_iris_embeddings.pb"].as_slice())?
            .embedding_v1
            .ok_or("missing DI record")?;
        assert_eq!(di.model_version, "synthetic-model");
        assert_eq!(di.embedding_version, "synthetic-embedding");
        assert_eq!(di.embedding_inference_backend, "none");
        assert_eq!(di.left_embedding, [-1, 2]);
        assert_eq!(di.left_mirror_embedding, [3, -4]);
        assert_eq!(di.right_embedding, [5, -6]);
        assert_eq!(di.right_mirror_embedding, [-7, 8]);
        assert_eq!(di.left_embedding_f32, [1.0, 2.0]);
        assert_eq!(di.left_mirror_embedding_f32, [3.0, -4.0]);
        assert_eq!(di.right_embedding_f32, [5.0, -6.0]);
        assert_eq!(di.right_mirror_embedding_f32, [-7.0, 8.0]);
        for i in 0..3 {
            let shares: serde_json::Value =
                serde_json::from_slice(&tier0[&format!("iris_code_shares_{i}.json")])?;
            assert_eq!(shares["IRIS_version"], iris["IRIS_version"]);
            assert_eq!(shares["IRIS_shares_version"], "synthetic-sharing");
            for eye in ["left", "right"] {
                for code in ["iris", "mask"] {
                    assert_eq!(
                        shares[format!("{eye}_{code}_code_shares")],
                        format!("{eye}-{code}-{i}")
                    );
                }
            }
            let shares = DiIrisEmbeddingShares::decode(
                tier0[&format!("di_iris_embeddings_shares_{i}.pb")].as_slice(),
            )?
            .share_v1
            .ok_or("missing DI share record")?;
            assert_eq!(shares.model_version, di.model_version);
            assert_eq!(shares.embedding_version, di.embedding_version);
            assert_eq!(shares.shares_version, "synthetic-di-sharing");
            assert_eq!(shares.left_share, [10 + i]);
            assert_eq!(shares.left_mirror_share, [20 + i]);
            assert_eq!(shares.right_share, [30 + i]);
            assert_eq!(shares.right_mirror_share, [40 + i]);
        }
        assert_eq!(info["left_ir_image_id"], "synthetic-left");
        assert_eq!(info["right_ir_image_id"], "synthetic-right");
        assert_eq!(info["thumbnail_image_id"], "synthetic-thumbnail");
        assert_eq!(
            info["left_ir_multiframe_image_ids"],
            serde_json::json!(["synthetic-extra-left"])
        );
        assert_eq!(
            info["right_ir_multiframe_image_ids"],
            serde_json::json!(["synthetic-extra-right"])
        );
        assert_eq!(tier0.len(), 18);
        let archives = &tier0;
        for (name, key) in [
            ("iris.tar", &backends[0]),
            ("normalized_iris.tar", &backends[1]),
            ("face.tar", &backends[2]),
            ("fraud.tar", &backends[2]),
        ] {
            let inner = files(&open(&archives[name], key)?)?;
            if name == "iris.tar" {
                assert_eq!(inner.len(), 4);
                for id in ["synthetic-extra-left", "synthetic-extra-right"] {
                    assert_eq!(inner[&format!("{id}.png")], inner["left_ir.png"]);
                }
            }
            if name == "normalized_iris.tar" {
                assert_eq!(inner.len(), 24);
                assert!(inner
                    .keys()
                    .all(|name| !name.starts_with("synthetic-extra")));
            }
            for (name, bytes) in inner {
                if name.contains("commitment") || name.contains("blinding_factors") {
                    assert!(
                        !bytes.is_empty(),
                        "synthetic Hyrax output must not be empty"
                    );
                }
                assert!(
                    expected.insert(name, hash(&bytes)).is_none(),
                    "duplicate hash name"
                );
            }
        }
        let modalities =
            files(&open(&tier0["face_ir_and_thermal.tar"], &backends[3])?)?;
        assert_eq!(modalities.len(), 2);
        for (name, bytes) in modalities {
            assert!(
                expected.insert(name, hash(&bytes)).is_none(),
                "duplicate modality hash"
            );
        }
        for name in [
            "face_embeddings.json",
            "iris_codes.json",
            "iris_code_shares_0.json",
            "iris_code_shares_1.json",
            "iris_code_shares_2.json",
            "di_iris_embeddings.pb",
            "di_iris_embeddings_shares_0.pb",
            "di_iris_embeddings_shares_1.pb",
            "di_iris_embeddings_shares_2.pb",
        ] {
            expected.insert(name.to_owned(), hash(&tier0[name]));
        }
    }
    assert_eq!(
        manifest, expected,
        "manifest does not exactly cover package contents"
    );
    Ok(())
}
