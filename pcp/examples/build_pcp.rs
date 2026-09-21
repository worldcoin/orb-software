//! In-memory synthetic package construction and round-trip checks, not an
//! untrusted-package verifier or a production signer implementation.

use std::{collections::BTreeMap, error::Error, io::Read, time::SystemTime};

use data_encoding::{BASE64, HEXLOWER};
use flate2::read::GzDecoder;
use orb_pcp::{builder, inner, metadata, payload};
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
    let included = builder::Included {
        images: inner::Images {
            left: Some(inner::Eye {
                primary: frame("synthetic-left", &png, &normalized),
                multiframe: &[],
            }),
            right: Some(inner::Eye {
                primary: frame("synthetic-right", &png, &normalized),
                multiframe: &[],
            }),
            thumbnail_png: Some(&png),
            face_ir_png: Some(&png),
            thermal_png: Some(&png),
            fraud: Some(inner::Fraud {
                scc_rgb_png: &png,
                left_rgb_png: &png,
                right_rgb_png: &png,
                left_thermal_png: Some(&png),
                right_thermal_png: Some(&png),
                scc_depth_png: Some(&png),
                left_depth_png: Some(&png),
                right_depth_png: Some(&png),
            }),
        },
        thumbnail_image_id: Some("synthetic-thumbnail"),
        left_iris_code_aggregate_image_ids: &["synthetic-left"],
        right_iris_code_aggregate_image_ids: &["synthetic-right"],
        face_embeddings: &[payload::FaceEmbedding {
            embedding: "synthetic-embedding",
            embedding_type: "synthetic",
            embedding_version: "example",
            embedding_inference_backend: "none",
        }],
        iris_codes: payload::IrisCodes {
            iris_version: Some("synthetic"),
            left_iris_code: Some("synthetic-left-iris"),
            left_mask_code: Some("synthetic-left-mask"),
            right_iris_code: Some("synthetic-right-iris"),
            right_mask_code: Some("synthetic-right-mask"),
        },
        iris_shares: builder::IrisShares {
            version: "synthetic",
            left_iris: ["synthetic-share"; 3],
            left_mask: ["synthetic-share"; 3],
            right_iris: ["synthetic-share"; 3],
            right_mask: ["synthetic-share"; 3],
        },
        di_left: None,
        di_right: None,
        di_shares_version: "synthetic",
    };

    for version in [
        builder::Version::V2_7,
        builder::Version::V2_8,
        builder::Version::V3_0,
    ] {
        for redacted in [false, true] {
            let user = box_::gen_keypair();
            let backends: [KeyPair; 4] = std::array::from_fn(|_| box_::gen_keypair());
            let encrypted_keys: [String; 4] = std::array::from_fn(|index| {
                BASE64.encode(&sealedbox::seal(backends[index].1.as_ref(), &user.0))
            });
            let request = builder::Request {
                version,
                timestamp: TIMESTAMP,
                info: metadata::Info {
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
                    device_public_key: (version != builder::Version::V2_7)
                        .then_some("synthetic-device-key"),
                },
                user_public_key: &user.0 .0,
                backend_keys: payload::BackendKeys {
                    iris: backend_key(&backends[0], &encrypted_keys[0]),
                    normalized_iris: backend_key(&backends[1], &encrypted_keys[1]),
                    face: backend_key(&backends[2], &encrypted_keys[2]),
                    tier2: backend_key(&backends[3], &encrypted_keys[3]),
                },
                biometrics: if redacted {
                    builder::Biometrics::Redacted
                } else {
                    builder::Biometrics::Included(&included)
                },
            };
            // P-256 is only this example's caller-owned signer choice.
            let signer = SigningKey::random(&mut OsRng);
            let mut calls = 0;
            let package = builder::build(&request, &mut OsRng, |hash| {
                calls += 1;
                let signature: Signature = signer.sign_prehash(hash)?;
                Ok::<_, p256::ecdsa::Error>(signature.to_der().as_bytes().to_vec())
            })?;
            assert_eq!(calls, 1);
            verify(&package, version, redacted, &user, &backends, &signer)?;
            println!(
                "{version:?} redacted={redacted}: encrypted tier lengths [{}, {}, {}]; verified",
                package.tier0.len(), package.tier1.len(), package.tier2.len()
            );
        }
    }
    Ok(())
}

fn frame<'a>(id: &'a str, png: &'a [u8], data: &'a [u8]) -> inner::Frame<'a> {
    inner::Frame {
        image_id: id,
        ir_png: png,
        normalized: inner::NormalizedFrame {
            image: data,
            mask: data,
            image_resized: data,
            mask_resized: data,
        },
    }
}

fn backend_key<'a>(pair: &'a KeyPair, encrypted: &'a str) -> payload::BackendKey<'a> {
    payload::BackendKey {
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
    package: &builder::Package,
    version: builder::Version,
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
    assert_eq!(
        expected.len(),
        if version == builder::Version::V2_7 {
            9
        } else {
            10
        }
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
        builder::Version::V2_7 => "2.7",
        builder::Version::V2_8 => "2.8",
        builder::Version::V3_0 => {
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
        info.get("device_public_key").is_some(),
        version != builder::Version::V2_7
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
        assert!(info["left_ir_image_id"] == "synthetic-left");
        assert!(info["right_ir_image_id"] == "synthetic-right");
        assert!(info["thumbnail_image_id"] == "synthetic-thumbnail");
        let archives = if version == builder::Version::V3_0 {
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
        let modalities = if version == builder::Version::V3_0 {
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
