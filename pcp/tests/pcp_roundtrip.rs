#![cfg(feature = "not-prod-diagnostics")]

use color_eyre::{
    eyre::{bail, eyre, OptionExt, WrapErr},
    Result,
};
use data_encoding::{BASE64, HEXLOWER};
use flate2::read::GzDecoder;
use orb_pcp::{self as pcp, v1};
use orb_pcp_defs::prost::Message;
use p256::ecdsa::{
    signature::hazmat::{PrehashSigner, PrehashVerifier},
    Signature, SigningKey,
};
use rand::rngs::OsRng;
use ring::digest::{digest, SHA256};
use std::{collections::BTreeMap, io::Read, path::Path};

type Files = BTreeMap<String, Vec<u8>>;

const INNER_ARCHIVES: [&str; 4] = [
    "iris.tar",
    "normalized_iris.tar",
    "face.tar",
    "face_ir_and_thermal.tar",
];

const SALTED_FIELDS: [&str; 10] = [
    "signup_id",
    "signup_reason",
    "orb_id",
    "operator_id",
    "timestamp",
    "qr_code",
    "id_commitment",
    "software_version",
    "orb_country",
    "device_public_key",
];

#[test]
fn fixture_with_protobuf_round_trips() -> Result<()> {
    round_trip("pcp.with_protobuf.000acfb82de34f698b20dda500000000.tier0.tar.gz")
}

#[test]
fn fixture_without_protobuf_round_trips() -> Result<()> {
    round_trip("pcp.without_protobuf.000a23d15f2365b502a0106d00000000.tier0.tar.gz")
}

fn round_trip(filename: &str) -> Result<()> {
    // Arrange
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(filename);

    let bytes = std::fs::read(&path)
        .wrap_err_with(|| format!("could not read fixture {filename}"))?;

    if bytes.starts_with(b"version https://git-lfs.github.com/spec/v1") {
        bail!("fixture is an LFS pointer; run git lfs pull");
    }

    let (timestamp, original) = read_tier(&bytes, "tier0.tar.gz")?;
    verify_manifest(&original, "2.7")?;

    let iris_files = read_tar(&original["iris.tar"])?;
    let normalized = read_tar(&original["normalized_iris.tar"])?;
    let face = read_tar(&original["face.tar"])?;
    let extra = read_tar(&original["face_ir_and_thermal.tar"])?;

    let images = pcp::PackageImages {
        left: eye(&iris_files, &normalized, "left"),
        right: eye(&iris_files, &normalized, "right"),
        thumbnail_png: Some(&face["thumbnail.png"]),
        face_ir_png: Some(&extra["face_ir.png"]),
        thermal_png: Some(&extra["thermal.png"]),
        fraud: None,
    };

    let info: v1::Info = serde_json::from_slice(&original["info.json"])?;
    assert!(info.left_ir_multiframe_image_ids.is_empty());
    assert!(info.right_ir_multiframe_image_ids.is_empty());

    let face_embeddings: Vec<v1::FaceEmbedding> =
        serde_json::from_slice(&original["face_embeddings.json"])?;

    let codes: v1::IrisCodes = serde_json::from_slice(&original["iris_codes.json"])?;
    let embeddings =
        v1::DiIrisEmbeddings::decode(original["di_iris_embeddings.pb"].as_slice())?;

    let mut iris_shares = Vec::new();
    let mut di_shares = Vec::new();

    for i in 0..3 {
        iris_shares.push(serde_json::from_slice::<v1::IrisCodeShares>(
            &original[&format!("iris_code_shares_{i}.json")],
        )?);

        di_shares.push(v1::DiIrisEmbeddingShares::decode(
            original[&format!("di_iris_embeddings_shares_{i}.pb")].as_slice(),
        )?);
    }

    let iris = pcp::Iris::codes(Some(&codes)).shares([
        Some(&iris_shares[0]),
        Some(&iris_shares[1]),
        Some(&iris_shares[2]),
    ])?;

    let di = pcp::Di::embeddings(&embeddings).shares([
        &di_shares[0],
        &di_shares[1],
        &di_shares[2],
    ])?;

    let biometrics = pcp::Biometrics::images(&images)
        .iris(iris)
        .di(di)
        .face_embeddings(&face_embeddings);

    let keys: v1::BackendKeys = serde_json::from_slice(&original["backend_keys.json"])?;
    let mut decoded_keys = Vec::new();

    for key in [&keys.face, &keys.iris, &keys.normalized_iris, &keys.tier2] {
        let key = key.as_ref().ok_or_eyre("missing backend key")?;

        let public_key: [u8; 32] = BASE64
            .decode(
                key.public_key
                    .as_ref()
                    .ok_or_eyre("missing public key")?
                    .as_bytes(),
            )?
            .try_into()
            .map_err(|_| eyre!("backend public key must be 32 bytes"))?;

        let encrypted_private_key = key
            .encrypted_private_key
            .as_deref()
            .ok_or_eyre("missing encrypted private key")?;

        decoded_keys.push((public_key, encrypted_private_key));
    }

    let backend = |i: usize| pcp::BackendKey {
        public_key: &decoded_keys[i].0,
        encrypted_private_key: decoded_keys[i].1,
    };

    let backend_keys = pcp::BackendKeys::face(backend(0))
        .iris(backend(1))
        .normalized_iris(backend(2))
        .tier2(backend(3));

    let signer = SigningKey::random(&mut OsRng);

    // Act
    let rebuilt = pcp::Builder::timestamp(timestamp)
        .info(&info)
        .user_public_key(&[0; 32]) // Diagnostics do not encrypt the output.
        .backend_keys(backend_keys)
        .biometrics(pcp::BiometricPolicy::Included(biometrics))
        .rng(&mut OsRng)
        .signer(|hash| {
            let signature: Signature = signer.sign_prehash(hash)?;

            Ok::<_, p256::ecdsa::Error>(signature.to_der().as_bytes().to_vec())
        })
        .build_unencrypted()?;

    // Assert
    let (rebuilt_timestamp, rebuilt_files) = read_tier(&rebuilt.tier0, "tier0.tar.gz")?;
    assert_eq!(rebuilt_timestamp, timestamp);

    compare_files(&original, &rebuilt_files)?;
    compare_info(&original["info.json"], &rebuilt_files["info.json"])?;
    verify_manifest(&rebuilt_files, "2.8")?;

    let signature = Signature::from_der(&rebuilt_files["hashes.sign"])?;
    signer.verifying_key().verify_prehash(
        digest(&SHA256, &rebuilt_files["hashes.json"]).as_ref(),
        &signature,
    )?;

    for (name, bytes) in [
        ("tier1.tar.gz", &rebuilt.tier1),
        ("tier2.tar.gz", &rebuilt.tier2),
    ] {
        let (mtime, files) = read_tier(bytes, name)?;
        assert_eq!(mtime, timestamp);
        assert!(files.is_empty(), "{name} should be empty");
    }

    Ok(())
}

fn read_tar(bytes: &[u8]) -> Result<Files> {
    let mut files = Files::new();

    for entry in tar::Archive::new(bytes).entries()? {
        let mut entry = entry?;
        let name = entry
            .path()?
            .to_str()
            .ok_or_eyre("non-UTF-8 filename")?
            .to_owned();

        let header = entry.header();
        assert!(header.entry_type().is_file(), "not a regular file: {name}");

        let mut data = Vec::new();
        entry.read_to_end(&mut data)?;

        assert!(
            files.insert(name.clone(), data).is_none(),
            "duplicate file: {name}"
        );
    }

    Ok(files)
}

fn read_tier(bytes: &[u8], name: &str) -> Result<(u32, Files)> {
    let mut decoder = GzDecoder::new(bytes);
    let header = decoder.header().ok_or_eyre("missing gzip header")?;
    assert_eq!(header.filename(), Some(name.as_bytes()));

    let timestamp = header.mtime();

    let mut tar = Vec::new();
    decoder.read_to_end(&mut tar)?;

    Ok((timestamp, read_tar(&tar)?))
}

fn eye<'a>(iris: &'a Files, normalized: &'a Files, side: &str) -> pcp::IrisEye<'a> {
    pcp::IrisEye {
        primary: pcp::PrimaryIrisFrame {
            ir_png: &iris[&format!("{side}_ir.png")],
            normalized: pcp::NormalizedIrisFrame {
                image: &normalized[&format!("{side}_normalized_image.bin")],
                mask: &normalized[&format!("{side}_normalized_mask.bin")],
                image_resized: &normalized
                    [&format!("{side}_normalized_image_resized.bin")],
                mask_resized: &normalized
                    [&format!("{side}_normalized_mask_resized.bin")],
            },
        },
        multiframe: &[],
    }
}

fn is_hyrax_file(name: &str) -> bool {
    let Some(name) = name
        .strip_prefix("left_")
        .or_else(|| name.strip_prefix("right_"))
    else {
        return false;
    };

    matches!(
        name,
        "normalized_image_commitment.bin"
            | "normalized_image_blinding_factors.bin"
            | "normalized_mask_commitment.bin"
            | "normalized_mask_blinding_factors.bin"
            | "normalized_image_commitment_resized.bin"
            | "normalized_image_blinding_factors_resized.bin"
            | "normalized_mask_commitment_resized.bin"
            | "normalized_mask_blinding_factors_resized.bin"
    )
}

fn compare_files(original: &Files, rebuilt: &Files) -> Result<()> {
    assert!(
        original.keys().eq(rebuilt.keys()),
        "archive filenames differ"
    );

    for (name, bytes) in original {
        if INNER_ARCHIVES.contains(&name.as_str()) {
            let original_inner = read_tar(bytes)?;
            let rebuilt_inner = read_tar(&rebuilt[name])?;

            assert!(
                original_inner.keys().eq(rebuilt_inner.keys()),
                "filenames differ in {name}"
            );

            for (file, bytes) in &original_inner {
                if name == "normalized_iris.tar" && is_hyrax_file(file) {
                    assert!(!bytes.is_empty(), "empty fixture Hyrax file: {file}");
                    assert_eq!(
                        bytes.len(),
                        rebuilt_inner[file].len(),
                        "Hyrax size: {file}"
                    );
                } else {
                    assert!(
                        bytes == &rebuilt_inner[file],
                        "contents differ: {name}/{file}"
                    );
                }
            }
        } else if !matches!(name.as_str(), "info.json" | "hashes.json" | "hashes.sign")
        {
            assert!(bytes == &rebuilt[name], "contents differ: {name}");
        }
    }

    Ok(())
}

fn compare_info(original: &[u8], rebuilt: &[u8]) -> Result<()> {
    let mut original: serde_json::Value = serde_json::from_slice(original)?;
    let mut rebuilt: serde_json::Value = serde_json::from_slice(rebuilt)?;

    for field in SALTED_FIELDS {
        original
            .as_object_mut()
            .ok_or_eyre("info is not an object")?
            .remove(&format!("{field}_salt"));

        rebuilt
            .as_object_mut()
            .ok_or_eyre("info is not an object")?
            .remove(&format!("{field}_salt"));
    }

    assert!(
        original == rebuilt,
        "metadata differs beyond regenerated salts"
    );

    Ok(())
}

fn hash(bytes: &[u8]) -> String {
    HEXLOWER.encode(digest(&SHA256, bytes).as_ref())
}

fn verify_manifest(files: &Files, version: &str) -> Result<()> {
    let info: serde_json::Value = serde_json::from_slice(&files["info.json"])?;
    let mut expected = BTreeMap::new();

    for field in SALTED_FIELDS {
        let salt_name = format!("{field}_salt");

        if let Some(value) = info.get(field) {
            let value = value.as_str().ok_or_eyre("salted field is not a string")?;
            let salt = info[&salt_name].as_str().ok_or_eyre("missing salt")?;
            let decoded = HEXLOWER.decode(salt.as_bytes())?;

            assert_eq!(decoded.len(), 16, "salt length: {field}");

            expected
                .insert(field.to_owned(), hash(format!("{value}{salt}").as_bytes()));
        } else {
            assert!(
                info.get(&salt_name).is_none(),
                "field {field} is missing, but its salt is present"
            );
        }
    }

    for (name, bytes) in files {
        if INNER_ARCHIVES.contains(&name.as_str()) {
            for (file, data) in read_tar(bytes)? {
                assert!(
                    expected.insert(file.clone(), hash(&data)).is_none(),
                    "duplicate manifest entry: {file}"
                );
            }
        } else if !matches!(name.as_str(), "info.json" | "hashes.json" | "hashes.sign")
        {
            assert!(
                expected.insert(name.clone(), hash(bytes)).is_none(),
                "duplicate manifest entry: {name}"
            );
        }
    }

    expected.insert("version".into(), version.into());

    let actual: BTreeMap<String, String> =
        serde_json::from_slice(&files["hashes.json"])?;

    assert!(
        expected == actual,
        "manifest does not match package contents"
    );

    Ok(())
}
