//! In-memory tar and gzip encoding for PCP payloads and tiers. Owned archive
//! buffers are zeroized on drop.

use std::io::Write;

use orb_wld_data_id::ImageId;
use zeroize::Zeroizing;

use crate::{
    crypto, manifest,
    payload::{EncodedDaugman, EncodedDi},
};

#[derive(Debug, thiserror::Error)]
pub enum ArchiveError {
    #[error("invalid archive filename")]
    InvalidName,
    #[error("timestamp exceeds the gzip 32-bit seconds field")]
    TimestampOutOfRange,
    #[error("archive encoding failed")]
    Io(#[from] std::io::Error),
}

/// Encodes regular files in the supplied order using the PCP GNU tar headers.
///
/// The timestamp is seconds since the Unix epoch, shared by all entries.
/// Headers use uid/gid 0, mode 0644, and device major/minor 0. Input buffers
/// are borrowed; the returned archive owns a copy of their bytes.
///
/// Names must be nonempty single components, at most 100 UTF-8 bytes, without
/// `/`, `\`, `:`, or control characters; `.` and `..` are rejected. The manifest
/// rejects duplicate names.
pub(crate) fn encode_tar<'a>(
    timestamp: u64,
    entries: impl IntoIterator<Item = (&'a str, &'a [u8])>,
) -> Result<Zeroizing<Vec<u8>>, ArchiveError> {
    let mut bytes = Zeroizing::new(Vec::new());
    let mut archive = tar::Builder::new(&mut *bytes);
    for (name, data) in entries {
        validate_name(name)?;
        // The GNU header's NUL regular-file type is part of the package bytes.
        let mut header = tar::Header::new_gnu();
        header.set_path(name)?;
        header.set_size(data.len() as u64);
        header.set_mtime(timestamp);
        header.set_uid(0);
        header.set_gid(0);
        header.set_mode(0o644);
        header.set_device_major(0)?;
        header.set_device_minor(0)?;
        header.set_cksum();
        archive.append(&header, data)?;
    }
    archive.into_inner()?;
    Ok(bytes)
}

/// Gzip-encodes bytes at the best compression level with explicit header metadata.
///
/// The timestamp is seconds since the Unix epoch and must fit in `u32`.
/// The filename follows the same rules as [`encode_tar`]. Compressed bytes depend
/// on the compression backend; the header metadata and decompressed bytes are
/// stable.
pub(crate) fn compress(
    data: &[u8],
    timestamp: u64,
    filename: &str,
) -> Result<Zeroizing<Vec<u8>>, ArchiveError> {
    validate_name(filename)?;
    let timestamp =
        u32::try_from(timestamp).map_err(|_| ArchiveError::TimestampOutOfRange)?;
    let mut bytes = Zeroizing::new(Vec::new());
    let mut encoder = flate2::GzBuilder::new()
        .filename(filename)
        .mtime(timestamp)
        .write(&mut *bytes, flate2::Compression::best());
    encoder.write_all(data)?;
    encoder.finish()?;
    Ok(bytes)
}

fn validate_name(name: &str) -> Result<(), ArchiveError> {
    if name.is_empty()
        || name.len() > 100
        || matches!(name, "." | "..")
        || name
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\' | ':'))
    {
        return Err(ArchiveError::InvalidName);
    }
    Ok(())
}

pub struct NormalizedIrisFrame<'a> {
    pub image: &'a [u8],
    pub mask: &'a [u8],
    pub image_resized: &'a [u8],
    pub mask_resized: &'a [u8],
}

/// Written as `left_ir.png`/`right_ir.png`, with its normalization as
/// `{left,right}_normalized_*.bin`. Its image ID is the eye's `*_ir_image_id`
/// in `info.json`.
pub struct PrimaryIrisFrame<'a> {
    pub ir_png: &'a [u8],
    pub normalized: NormalizedIrisFrame<'a>,
}

/// An additional IR capture of the eye, written as `{image_id}.png` and listed in
/// the eye's `*_ir_multiframe_image_ids` in `info.json`.
pub struct IrisFrame<'a> {
    pub image_id: &'a ImageId,
    pub ir_png: &'a [u8],
    /// Normalization of this frame from the multiframe iris pipeline, written with
    /// its Hyrax commitments as `{image_id}_normalized_*.bin`. Without it, only
    /// `{image_id}.png` is written.
    pub normalized: Option<NormalizedIrisFrame<'a>>,
}

/// One eye's IR captures: the primary frame and any multiframe captures, in
/// capture order. The `*_iris_code_aggregate_image_ids` in `info.json` name the
/// frames an aggregated iris code was computed from and are written as given.
pub struct IrisEye<'a> {
    pub primary: PrimaryIrisFrame<'a>,
    pub multiframe: &'a [IrisFrame<'a>],
}

/// Images written to `fraud.tar`; each `None` image is left out.
pub struct FraudImages<'a> {
    pub scc_rgb_png: Option<&'a [u8]>,
    pub left_rgb_png: Option<&'a [u8]>,
    pub right_rgb_png: Option<&'a [u8]>,
    pub left_thermal_png: Option<&'a [u8]>,
    pub right_thermal_png: Option<&'a [u8]>,
    pub scc_depth_png: Option<&'a [u8]>,
    pub left_depth_png: Option<&'a [u8]>,
    pub right_depth_png: Option<&'a [u8]>,
}

/// Encoded PNGs and normalized bytes.
pub struct PackageImages<'a> {
    pub left: IrisEye<'a>,
    pub right: IrisEye<'a>,
    /// `None` writes an empty `thumbnail.png`.
    pub thumbnail_png: Option<&'a [u8]>,
    /// `None` leaves the file out of `face_ir_and_thermal.tar`.
    pub face_ir_png: Option<&'a [u8]>,
    /// `None` leaves the file out of `face_ir_and_thermal.tar`.
    pub thermal_png: Option<&'a [u8]>,
    /// `None` leaves out `fraud.tar`.
    pub fraud: Option<FraudImages<'a>>,
}

pub(crate) struct InnerArchives {
    pub iris: Zeroizing<Vec<u8>>,
    pub normalized_iris: Zeroizing<Vec<u8>>,
    pub face: Zeroizing<Vec<u8>>,
    pub fraud: Option<Zeroizing<Vec<u8>>>,
    pub face_ir_and_thermal: Zeroizing<Vec<u8>>,
    // Keep repeated names until the manifest validates them; a map would lose them.
    pub hashes: Vec<(String, [u8; 32])>,
}

#[derive(Debug, thiserror::Error)]
pub enum InnerArchiveError {
    #[error("inner archive encoding failed")]
    Archive(#[from] ArchiveError),
    #[error("normalized iris commitment generation failed")]
    Commitment(#[from] crypto::CommitmentError),
}

/// Builds the inner archives and hashes their individual files.
pub(crate) fn encode_inner(
    timestamp: u64,
    images: &PackageImages<'_>,
    rng: &mut (impl rand::RngCore + rand::CryptoRng),
) -> Result<InnerArchives, InnerArchiveError> {
    let (left, right) = (&images.left, &images.right);
    let left_normalized = &left.primary.normalized;
    let right_normalized = &right.primary.normalized;
    let mut hashes = Vec::new();

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
    // Commitments draw randomness in this order, so seeded RNGs reproduce them.
    for resized in [false, true] {
        for (prefix, frame) in [("left", left_normalized), ("right", right_normalized)]
        {
            normalized_files.extend(normalized_pair(prefix, frame, resized, rng)?);
        }
    }
    for frame in left.multiframe.iter().chain(right.multiframe) {
        let Some(normalized) = &frame.normalized else {
            continue;
        };
        let prefix = frame.image_id.to_string();
        for resized in [false, true] {
            normalized_files
                .extend(normalized_pair(&prefix, normalized, resized, rng)?);
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
                    ("scc_rgb.png", fraud.scc_rgb_png),
                    ("left_rgb.png", fraud.left_rgb_png),
                    ("right_rgb.png", fraud.right_rgb_png),
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

    Ok(InnerArchives {
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
    generated: crypto::GeneratedCommitment<'a>,
}

fn normalized_pair<'a>(
    prefix: &str,
    frame: &NormalizedIrisFrame<'a>,
    resized: bool,
    rng: &mut (impl rand::RngCore + rand::CryptoRng),
) -> Result<[NormalizedFile<'a>; 2], InnerArchiveError> {
    let suffix = if resized { "_resized" } else { "" };
    let (image, mask) = if resized {
        (frame.image_resized, frame.mask_resized)
    } else {
        (frame.image, frame.mask)
    };
    let mut make_file = |kind, data| -> Result<NormalizedFile<'a>, InnerArchiveError> {
        Ok(NormalizedFile {
            names: [
                format!("{prefix}_normalized_{kind}{suffix}.bin"),
                format!("{prefix}_normalized_{kind}_commitment{suffix}.bin"),
                format!("{prefix}_normalized_{kind}_blinding_factors{suffix}.bin"),
            ],
            generated: crypto::generate_commitment(data, rng)?,
        })
    };
    Ok([make_file("image", image)?, make_file("mask", mask)?])
}

fn encode_archive<'a>(
    timestamp: u64,
    files: impl IntoIterator<Item = (&'a str, &'a [u8])>,
    hashes: &mut Vec<(String, [u8; 32])>,
) -> Result<Zeroizing<Vec<u8>>, InnerArchiveError> {
    let files: Vec<_> = files.into_iter().collect();
    let archive = encode_tar(timestamp, files.iter().copied())?;
    for (name, data) in files {
        hashes.push((name.to_owned(), manifest::sha256(data)));
    }
    Ok(archive)
}

/// Backend-encrypted inner archives.
pub(crate) struct BiometricArchives<'a> {
    pub iris_sealed: &'a [u8],
    pub normalized_iris_sealed: &'a [u8],
    pub face_sealed: &'a [u8],
    pub fraud_sealed: Option<&'a [u8]>,
    pub face_ir_and_thermal_sealed: &'a [u8],
}

/// All biometric entries, including serialized payloads. Empty payloads are
/// written as empty files; iris code files that were not supplied are left out.
pub(crate) struct PreparedBiometricFiles<'a> {
    pub archives: BiometricArchives<'a>,
    pub face_embeddings_json: &'a [u8],
    pub daugman: &'a EncodedDaugman,
    pub di: &'a EncodedDi,
}

/// Files written to tier 0 with or without biometrics. `hashes_signature` signs
/// the exact `hashes_json` bytes.
pub(crate) struct Tier0Files<'a> {
    pub info_json: &'a [u8],
    pub hashes_json: &'a [u8],
    pub hashes_signature: &'a [u8],
    pub backend_keys_json: &'a [u8],
    /// Follows `info.json` when present.
    pub migration_pb: Option<&'a [u8]>,
}

/// Encodes the tier 0 tar. The inner archives and biometric payloads are
/// included when `biometrics` is `Some`.
pub(crate) fn tier0(
    timestamp: u64,
    files: Tier0Files<'_>,
    biometrics: Option<&PreparedBiometricFiles<'_>>,
) -> Result<Zeroizing<Vec<u8>>, ArchiveError> {
    let mut entries = Vec::new();
    if let Some(biometrics) = biometrics {
        let archives = &biometrics.archives;
        entries.extend([
            ("iris.tar", archives.iris_sealed),
            ("normalized_iris.tar", archives.normalized_iris_sealed),
            ("face.tar", archives.face_sealed),
        ]);
        if let Some(fraud) = archives.fraud_sealed {
            entries.push(("fraud.tar", fraud));
        }
        entries.push((
            "face_ir_and_thermal.tar",
            archives.face_ir_and_thermal_sealed,
        ));
    }
    entries.push(("info.json", files.info_json));
    if let Some(migration_pb) = files.migration_pb {
        entries.push(("migration.pb", migration_pb));
    }
    if let Some(biometrics) = biometrics {
        entries.push(("face_embeddings.json", biometrics.face_embeddings_json));
        if let Some(codes) = &biometrics.daugman.codes {
            entries.push(("iris_codes.json", codes.as_slice()));
        }
        entries.extend(
            [
                "iris_code_shares_0.json",
                "iris_code_shares_1.json",
                "iris_code_shares_2.json",
            ]
            .into_iter()
            .zip(&biometrics.daugman.shares)
            .filter_map(|(name, share)| Some((name, share.as_deref()?))),
        );
        entries.push(("di_iris_embeddings.pb", biometrics.di.embeddings.as_slice()));
        entries.extend(
            [
                "di_iris_embeddings_shares_0.pb",
                "di_iris_embeddings_shares_1.pb",
                "di_iris_embeddings_shares_2.pb",
            ]
            .into_iter()
            .zip(biometrics.di.shares.iter().map(Vec::as_slice)),
        );
    }
    entries.extend([
        ("hashes.sign", files.hashes_signature),
        ("hashes.json", files.hashes_json),
        ("backend_keys.json", files.backend_keys_json),
    ]);
    encode_tar(timestamp, entries)
}

#[cfg(test)]
#[path = "../tests/unit/archive.rs"]
mod tests;
