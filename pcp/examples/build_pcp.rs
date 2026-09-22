//! In-memory synthetic package construction and round-trip checks, not an
//! untrusted-package verifier or a production signer implementation.
//!
//! Run with `cargo run -p orb-pcp --example build_pcp`. Builds and verifies
//! 2.7, 2.8, and 3.0 (with and without a device key), included and redacted.
//! Everything stays in memory; only case labels and encrypted sizes are printed.

use std::{collections::BTreeMap, error::Error, io::Read, time::SystemTime};

use data_encoding::{BASE64, HEXLOWER};
use flate2::read::GzDecoder;
use orb_pcp as pcp;
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
use sodiumoxide::crypto::{box_, sealedbox};

type Files = BTreeMap<String, Vec<u8>>;
type KeyPair = (box_::PublicKey, box_::SecretKey);
type Result<T> = std::result::Result<T, Box<dyn Error>>;

const TIMESTAMP: u64 = 1_700_000_000;
// A synthetic 1x1 white grayscale/alpha PNG, never a captured image.
const PNG_BASE64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+ip1sAAAAASUVORK5CYII=";

fn main() -> Result<()> {
    run()
}

/// Also exercised by the integration test without filesystem or service access.
pub fn run() -> Result<()> {
    sodiumoxide::init().map_err(|()| "libsodium initialization failed")?;
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
            primary: frame("synthetic-left", &png, &normalized),
            multiframe: &extra_left,
        }),
        right: Some(pcp::IrisEye {
            primary: frame("synthetic-right", &png, &normalized),
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
    let daugman = pcp::DaugmanData {
        iris_version: Some("synthetic"),
        shares_version: "synthetic-sharing",
        left: pcp::DaugmanEyeData {
            iris_code: Some("synthetic-left-iris"),
            mask_code: Some("synthetic-left-mask"),
            iris_code_shares: ["left-iris-0", "left-iris-1", "left-iris-2"],
            mask_code_shares: ["left-mask-0", "left-mask-1", "left-mask-2"],
        },
        right: pcp::DaugmanEyeData {
            iris_code: Some("synthetic-right-iris"),
            mask_code: Some("synthetic-right-mask"),
            iris_code_shares: ["right-iris-0", "right-iris-1", "right-iris-2"],
            mask_code_shares: ["right-mask-0", "right-mask-1", "right-mask-2"],
        },
    };
    let di = pcp::DiData {
        model_version: "synthetic-model",
        embedding_version: "synthetic-embedding",
        inference_backend: "none",
        shares_version: "synthetic-di-sharing",
        left: Some(pcp::DiEyeData {
            embedding: &[-1, 2],
            mirror_embedding: &[3, -4],
            embedding_f32: &[1.0, 2.0],
            mirror_embedding_f32: &[3.0, -4.0],
            embedding_shares: [&[10], &[11], &[12]],
            mirror_embedding_shares: [&[20], &[21], &[22]],
        }),
        right: Some(pcp::DiEyeData {
            embedding: &[5, -6],
            mirror_embedding: &[-7, 8],
            embedding_f32: &[5.0, -6.0],
            mirror_embedding_f32: &[-7.0, 8.0],
            embedding_shares: [&[30], &[31], &[32]],
            mirror_embedding_shares: [&[40], &[41], &[42]],
        }),
    };

    // Callers select the exact version and supply their actual metadata.
    // Archive/encryption/manifest choices belong to the builder, not the caller.
    for (version, device_public_key) in [
        (pcp::PcpVersion::V2_7, None),
        (pcp::PcpVersion::V2_8, Some("synthetic-device-key")),
        (pcp::PcpVersion::V3_0, None),
        (pcp::PcpVersion::V3_0, Some("synthetic-device-key")),
    ] {
        for redacted in [false, true] {
            let user = box_::gen_keypair();
            let backends: [KeyPair; 4] = std::array::from_fn(|_| box_::gen_keypair());
            let encrypted_keys: [String; 4] = std::array::from_fn(|index| {
                BASE64.encode(&sealedbox::seal(backends[index].1.as_ref(), &user.0))
            });
            let request = pcp::BuildRequest {
                version,
                timestamp: TIMESTAMP,
                info: pcp::PackageInfo {
                    signup_id: "synthetic-signup",
                    signup_reason: "synthetic-example",
                    orb_id: "synthetic-orb",
                    operator_id: "synthetic-operator",
                    capture_start: SystemTime::UNIX_EPOCH
                        + std::time::Duration::from_secs(TIMESTAMP),
                    qr_code: "synthetic-qr",
                    id_commitment: "synthetic-id-commitment",
                    software_version: "synthetic-example",
                    orb_country: "XX",
                    orb_public_key_certificate:
                        b"synthetic-placeholder-not-a-certificate",
                    device_public_key,
                },
                user_public_key: &user.0 .0,
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
                        thumbnail_image_id: Some("synthetic-thumbnail"),
                        left_iris_code_aggregate_image_ids: &["synthetic-left"],
                        right_iris_code_aggregate_image_ids: &["synthetic-right"],
                        face_embeddings: &[pcp::FaceEmbedding {
                            embedding: "synthetic-embedding",
                            embedding_type: "synthetic",
                            embedding_version: "example",
                            embedding_inference_backend: "none",
                        }],
                        daugman: &daugman,
                        di: Some(&di),
                    }
                },
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
                version,
                device_public_key,
                redacted,
                &user,
                &backends,
                &signer,
            )?;
            println!(
                "{version:?} device_key={} redacted={redacted}: encrypted tier lengths [{}, {}, {}]; verified",
                device_public_key.is_some(), package.tier0.len(), package.tier1.len(), package.tier2.len()
            );
        }
    }
    Ok(())
}

fn frame<'a>(id: &'a str, png: &'a [u8], data: &'a [u8]) -> pcp::IrisFrame<'a> {
    pcp::IrisFrame {
        image_id: id,
        ir_png: png,
        normalized: Some(pcp::NormalizedIrisFrame {
            image: data,
            mask: data,
            image_resized: data,
            mask_resized: data,
        }),
    }
}

fn backend_key<'a>(pair: &'a KeyPair, encrypted: &'a str) -> pcp::BackendKey<'a> {
    pcp::BackendKey {
        public_key: &pair.0 .0,
        encrypted_private_key: encrypted,
    }
}

fn hash(bytes: &[u8]) -> String {
    HEXLOWER.encode(digest(&SHA256, bytes).as_ref())
}

fn open(ciphertext: &[u8], pair: &KeyPair) -> Result<Vec<u8>> {
    sealedbox::open(ciphertext, &pair.0, &pair.1)
        .map_err(|()| "sealed-box round-trip failed".into())
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
    assert!(
        header.filename() == Some(name.as_bytes()),
        "gzip filename mismatch"
    );
    let mut tar = Vec::new();
    gzip.read_to_end(&mut tar)?;
    files(&tar)
}

fn verify(
    package: &pcp::Package,
    version: pcp::PcpVersion,
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
        assert!(
            digest(&SHA256, bytes).as_ref() == checksum,
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
        assert!(
            keys[role]["public_key"] == BASE64.encode(pair.0.as_ref()),
            "backend public key mismatch"
        );
        let encrypted = keys[role]["encrypted_private_key"]
            .as_str()
            .ok_or("encrypted backend key is not a string")?;
        let recovered = open(&BASE64.decode(encrypted.as_bytes())?, user)?;
        assert!(recovered == pair.1.as_ref(), "backend private key mismatch");
    }
    expected.insert(
        "backend_keys.json".to_owned(),
        hash(&tier0["backend_keys.json"]),
    );
    let wire_version = match version {
        pcp::PcpVersion::V2_7 => "2.7",
        pcp::PcpVersion::V2_8 => "2.8",
        pcp::PcpVersion::V3_0 => {
            expected.insert("tier_1".to_owned(), hash(&package.tier1));
            expected.insert("tier_2".to_owned(), hash(&package.tier2));
            for name in ["tier_3", "tier_4", "tier_5"] {
                expected.insert(name.to_owned(), HEXLOWER.encode(&[0; 32]));
            }
            "3.0"
        }
    };
    expected.insert("version".to_owned(), wire_version.to_owned());
    assert_eq!(
        info.get("device_public_key"),
        device_public_key.map(serde_json::Value::from).as_ref()
    );
    if redacted {
        assert_eq!(tier0.len(), 4);
        assert!(tier1.is_empty() && tier2.is_empty());
        for name in [
            "left_ir_image_id",
            "right_ir_image_id",
            "thumbnail_image_id",
        ] {
            assert!(info[name] == "", "redacted image ID remains");
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
        assert!(info["left_ir_image_id"] == "synthetic-left");
        assert!(info["right_ir_image_id"] == "synthetic-right");
        assert!(info["thumbnail_image_id"] == "synthetic-thumbnail");
        assert_eq!(
            info["left_ir_multiframe_image_ids"],
            serde_json::json!(["synthetic-extra-left"])
        );
        assert_eq!(
            info["right_ir_multiframe_image_ids"],
            serde_json::json!(["synthetic-extra-right"])
        );
        let archives = if version == pcp::PcpVersion::V3_0 {
            assert_eq!(tier0.len(), 13);
            assert_eq!(tier1.len(), 4);
            assert_eq!(tier2.len(), 1);
            &tier1
        } else {
            assert_eq!(tier0.len(), 18);
            assert!(tier1.is_empty() && tier2.is_empty());
            &tier0
        };
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
                    assert!(inner[&format!("{id}.png")] == inner["left_ir.png"]);
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
        let modalities = if version == pcp::PcpVersion::V3_0 {
            files(&tier2["face_ir_and_thermal.tar"])?
        } else {
            files(&open(&tier0["face_ir_and_thermal.tar"], &backends[3])?)?
        };
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
    assert!(
        manifest == expected,
        "manifest does not exactly cover package contents"
    );
    Ok(())
}
