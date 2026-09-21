//! Plaintext inner archives from consumer-encoded PNGs and normalized iris data.
//!
//! PNG encoding and image identifiers belong to the consumer. This module does
//! not decode images. Returned archives contain sensitive data and are not
//! automatically zeroized.

use std::collections::BTreeMap;

use crate::{archive, commitment};

pub struct NormalizedFrame<'a> {
    pub image: &'a [u8],
    pub mask: &'a [u8],
    pub image_resized: &'a [u8],
    pub mask_resized: &'a [u8],
}

pub struct Frame<'a> {
    pub image_id: &'a str,
    pub ir_png: &'a [u8],
    pub normalized: NormalizedFrame<'a>,
}

pub struct Eye<'a> {
    pub primary: Frame<'a>,
    pub multiframe: &'a [Frame<'a>],
}

pub struct Fraud<'a> {
    pub scc_rgb_png: &'a [u8],
    pub left_rgb_png: &'a [u8],
    pub right_rgb_png: &'a [u8],
    pub left_thermal_png: Option<&'a [u8]>,
    pub right_thermal_png: Option<&'a [u8]>,
    pub scc_depth_png: Option<&'a [u8]>,
    pub left_depth_png: Option<&'a [u8]>,
    pub right_depth_png: Option<&'a [u8]>,
}

pub struct Images<'a> {
    pub left: Option<Eye<'a>>,
    pub right: Option<Eye<'a>>,
    pub thumbnail_png: Option<&'a [u8]>,
    pub face_ir_png: Option<&'a [u8]>,
    pub thermal_png: Option<&'a [u8]>,
    pub fraud: Option<Fraud<'a>>,
}

pub struct Encoded {
    pub iris: Vec<u8>,
    pub normalized_iris: Vec<u8>,
    pub face: Vec<u8>,
    pub fraud: Option<Vec<u8>>,
    pub face_ir_and_thermal: Vec<u8>,
    pub hashes: BTreeMap<String, [u8; 32]>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("left eye is required by the current package format")]
    MissingLeftEye,
    #[error("right eye is required by the current package format")]
    MissingRightEye,
    #[error("duplicate inner archive manifest name")]
    DuplicateName,
    #[error("inner archive encoding failed")]
    Archive(#[from] archive::Error),
    #[error("normalized iris commitment generation failed")]
    Commitment(#[from] commitment::Error),
}

/// Builds legacy inner archives and hashes their individual files.
///
/// Both eyes are currently required. Optional fields prepare the input shape for
/// partial packages without changing the current wire format. An absent thumbnail
/// is an empty `thumbnail.png`; absent modalities yield an empty tar archive.
pub fn encode(
    timestamp: u64,
    images: &Images<'_>,
    rng: &mut (impl rand::RngCore + rand::CryptoRng),
) -> Result<Encoded, Error> {
    let left = images.left.as_ref().ok_or(Error::MissingLeftEye)?;
    let right = images.right.as_ref().ok_or(Error::MissingRightEye)?;
    let mut hashes = BTreeMap::new();

    let mut iris_files = vec![
        ("left_ir.png".to_owned(), left.primary.ir_png),
        ("right_ir.png".to_owned(), right.primary.ir_png),
    ];
    for frame in left.multiframe.iter().chain(right.multiframe) {
        iris_files.push((format!("{}.png", frame.image_id), frame.ir_png));
    }
    let iris = encode_archive(
        timestamp,
        iris_files.iter().map(|(name, data)| (name.as_str(), *data)),
        &mut hashes,
    )?;

    let mut normalized_files = Vec::new();
    // Generation order is part of deterministic compatibility with seeded callers.
    for resized in [false, true] {
        for (prefix, frame) in [
            ("left", &left.primary.normalized),
            ("right", &right.primary.normalized),
        ] {
            normalized_files.extend(normalized_pair(prefix, frame, resized, rng)?);
        }
    }
    for frame in left.multiframe.iter().chain(right.multiframe) {
        for resized in [false, true] {
            normalized_files.extend(normalized_pair(
                frame.image_id,
                &frame.normalized,
                resized,
                rng,
            )?);
        }
    }
    let normalized_iris = encode_archive(
        timestamp,
        normalized_files.iter().flat_map(|file| {
            [
                (file.names[0].as_str(), file.generated.data()),
                (file.names[1].as_str(), file.generated.commitment()),
                (file.names[2].as_str(), file.generated.blinding_factors()),
            ]
        }),
        &mut hashes,
    )?;
    let face = encode_archive(
        timestamp,
        [("thumbnail.png", images.thumbnail_png.unwrap_or_default())],
        &mut hashes,
    )?;
    let fraud = images
        .fraud
        .as_ref()
        .map(|fraud| {
            encode_archive(
                timestamp,
                [
                    ("scc_rgb.png", Some(fraud.scc_rgb_png)),
                    ("left_rgb.png", Some(fraud.left_rgb_png)),
                    ("right_rgb.png", Some(fraud.right_rgb_png)),
                    ("left_thermal.png", fraud.left_thermal_png),
                    ("right_thermal.png", fraud.right_thermal_png),
                    ("scc_depth.png", fraud.scc_depth_png),
                    ("left_depth.png", fraud.left_depth_png),
                    ("right_depth.png", fraud.right_depth_png),
                ]
                .into_iter()
                .filter_map(|(name, data)| data.map(|data| (name, data))),
                &mut hashes,
            )
        })
        .transpose()?;
    let face_ir_and_thermal = encode_archive(
        timestamp,
        [
            ("face_ir.png", images.face_ir_png),
            ("thermal.png", images.thermal_png),
        ]
        .into_iter()
        .filter_map(|(name, data)| data.map(|data| (name, data))),
        &mut hashes,
    )?;

    Ok(Encoded {
        iris,
        normalized_iris,
        face,
        fraud,
        face_ir_and_thermal,
        hashes,
    })
}

struct NormalizedFile<'a> {
    names: [String; 3],
    generated: commitment::Generated<'a>,
}

fn normalized_pair<'a>(
    prefix: &str,
    frame: &NormalizedFrame<'a>,
    resized: bool,
    rng: &mut (impl rand::RngCore + rand::CryptoRng),
) -> Result<[NormalizedFile<'a>; 2], Error> {
    let suffix = if resized { "_resized" } else { "" };
    let (image, mask) = if resized {
        (frame.image_resized, frame.mask_resized)
    } else {
        (frame.image, frame.mask)
    };
    let mut make_file = |kind, data| -> Result<NormalizedFile<'a>, Error> {
        Ok(NormalizedFile {
            names: [
                format!("{prefix}_normalized_{kind}{suffix}.bin"),
                format!("{prefix}_normalized_{kind}_commitment{suffix}.bin"),
                format!("{prefix}_normalized_{kind}_blinding_factors{suffix}.bin"),
            ],
            generated: commitment::generate(data, rng)?,
        })
    };
    Ok([make_file("image", image)?, make_file("mask", mask)?])
}

fn encode_archive<'a>(
    timestamp: u64,
    files: impl IntoIterator<Item = (&'a str, &'a [u8])>,
    hashes: &mut BTreeMap<String, [u8; 32]>,
) -> Result<Vec<u8>, Error> {
    let files: Vec<_> = files.into_iter().collect();
    let archive = archive::encode_tar(timestamp, files.iter().copied())?;
    for (name, data) in files {
        let digest = ring::digest::digest(&ring::digest::SHA256, data);
        let mut hash = [0; 32];
        hash.copy_from_slice(digest.as_ref());
        if hashes.insert(name.to_owned(), hash).is_some() {
            return Err(Error::DuplicateName);
        }
    }
    Ok(archive)
}
