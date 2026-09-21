//! In-memory tar and gzip encoding for PCP payloads and tiers.
//!
//! Returned buffers are not encrypted or automatically zeroized. Callers own
//! their sensitive-data handling; compression does not protect confidentiality.

use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
};

use crate::crypto;

#[derive(Debug, thiserror::Error)]
pub enum ArchiveError {
    #[error("invalid archive filename")]
    InvalidName,
    #[error("duplicate archive filename")]
    DuplicateName,
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
/// `/`, `\`, `:`, or control characters; `.` and `..` are rejected. Duplicate
/// names are rejected. The package layer must choose the protocol filenames
/// and entry order; this helper does not enforce a complete package layout.
pub(crate) fn encode_tar<'a>(
    timestamp: u64,
    entries: impl IntoIterator<Item = (&'a str, &'a [u8])>,
) -> Result<Vec<u8>, ArchiveError> {
    let mut archive = tar::Builder::new(Vec::new());
    let mut names = BTreeSet::new();
    for (name, data) in entries {
        validate_name(name)?;
        if !names.insert(name) {
            return Err(ArchiveError::DuplicateName);
        }
        // Preserve the GNU header's NUL regular-file type for byte compatibility.
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
    Ok(archive.into_inner()?)
}

/// Gzip-encodes bytes at the best compression level with explicit header metadata.
///
/// The timestamp is seconds since the Unix epoch and must fit in `u32`.
/// The filename follows the same rules as [`encode_tar`]. PCP tier filenames
/// are `tier0.tar.gz`, `tier1.tar.gz`, and `tier2.tar.gz`; inner archives remain
/// uncompressed. Compressed bytes may vary with the compression backend/version;
/// the header metadata and decompressed bytes are the compatibility contract.
pub(crate) fn compress(
    data: &[u8],
    timestamp: u64,
    filename: &str,
) -> Result<Vec<u8>, ArchiveError> {
    validate_name(filename)?;
    let timestamp =
        u32::try_from(timestamp).map_err(|_| ArchiveError::TimestampOutOfRange)?;
    let mut encoder = flate2::GzBuilder::new()
        .filename(filename)
        .mtime(timestamp)
        .write(Vec::new(), flate2::Compression::best());
    encoder.write_all(data)?;
    Ok(encoder.finish()?)
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

pub struct IrisFrame<'a> {
    pub image_id: &'a str,
    pub ir_png: &'a [u8],
    /// Required for primary frames; extra captures may have no normalized output.
    pub normalized: Option<NormalizedIrisFrame<'a>>,
}

pub struct IrisEye<'a> {
    pub primary: IrisFrame<'a>,
    pub multiframe: &'a [IrisFrame<'a>],
}

pub struct FraudImages<'a> {
    pub scc_rgb_png: &'a [u8],
    pub left_rgb_png: &'a [u8],
    pub right_rgb_png: &'a [u8],
    pub left_thermal_png: Option<&'a [u8]>,
    pub right_thermal_png: Option<&'a [u8]>,
    pub scc_depth_png: Option<&'a [u8]>,
    pub left_depth_png: Option<&'a [u8]>,
    pub right_depth_png: Option<&'a [u8]>,
}

/// Consumer-encoded PNGs and normalized bytes; image encoding stays with the caller.
/// Both eyes and their primary normalized data are required by the current wire
/// profile. Optional fields distinguish unavailable data from redaction; they
/// do not enable the future partial-package format.
pub struct PackageImages<'a> {
    pub left: Option<IrisEye<'a>>,
    pub right: Option<IrisEye<'a>>,
    /// Absence preserves an empty `thumbnail.png`, rather than omitting the file.
    pub thumbnail_png: Option<&'a [u8]>,
    /// Absent modalities are omitted from their inner archive; the archive remains.
    pub face_ir_png: Option<&'a [u8]>,
    /// Absent modalities are omitted from their inner archive; the archive remains.
    pub thermal_png: Option<&'a [u8]>,
    /// Absence omits the entire fraud archive.
    pub fraud: Option<FraudImages<'a>>,
}

pub(crate) struct InnerArchives {
    pub iris: Vec<u8>,
    pub normalized_iris: Vec<u8>,
    pub face: Vec<u8>,
    pub fraud: Option<Vec<u8>>,
    pub face_ir_and_thermal: Vec<u8>,
    pub hashes: BTreeMap<String, [u8; 32]>,
}

#[derive(Debug, thiserror::Error)]
pub enum InnerArchiveError {
    #[error("left eye is required by the current package format")]
    MissingLeftEye,
    #[error("right eye is required by the current package format")]
    MissingRightEye,
    #[error("left primary normalized data is required by the current package format")]
    MissingLeftNormalization,
    #[error("right primary normalized data is required by the current package format")]
    MissingRightNormalization,
    #[error("duplicate inner archive manifest name")]
    DuplicateName,
    #[error("inner archive encoding failed")]
    Archive(#[from] ArchiveError),
    #[error("normalized iris commitment generation failed")]
    Commitment(#[from] crypto::CommitmentError),
}

/// Builds legacy inner archives and hashes their individual files.
///
/// Both eyes are currently required. Optional fields prepare the input shape for
/// partial packages without changing the current wire format. An absent thumbnail
/// is an empty `thumbnail.png`; absent modalities yield an empty tar archive.
pub(crate) fn encode_inner(
    timestamp: u64,
    images: &PackageImages<'_>,
    rng: &mut (impl rand::RngCore + rand::CryptoRng),
) -> Result<InnerArchives, InnerArchiveError> {
    let left = images
        .left
        .as_ref()
        .ok_or(InnerArchiveError::MissingLeftEye)?;
    let right = images
        .right
        .as_ref()
        .ok_or(InnerArchiveError::MissingRightEye)?;
    let left_normalized = left
        .primary
        .normalized
        .as_ref()
        .ok_or(InnerArchiveError::MissingLeftNormalization)?;
    let right_normalized = right
        .primary
        .normalized
        .as_ref()
        .ok_or(InnerArchiveError::MissingRightNormalization)?;
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
        for (prefix, frame) in [("left", left_normalized), ("right", right_normalized)]
        {
            normalized_files.extend(normalized_pair(prefix, frame, resized, rng)?);
        }
    }
    for frame in left.multiframe.iter().chain(right.multiframe) {
        let Some(normalized) = &frame.normalized else {
            continue;
        };
        for resized in [false, true] {
            normalized_files.extend(normalized_pair(
                frame.image_id,
                normalized,
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
    hashes: &mut BTreeMap<String, [u8; 32]>,
) -> Result<Vec<u8>, InnerArchiveError> {
    let files: Vec<_> = files.into_iter().collect();
    let archive = encode_tar(timestamp, files.iter().copied())?;
    for (name, data) in files {
        let digest = ring::digest::digest(&ring::digest::SHA256, data);
        let mut hash = [0; 32];
        hash.copy_from_slice(digest.as_ref());
        if hashes.insert(name.to_owned(), hash).is_some() {
            return Err(InnerArchiveError::DuplicateName);
        }
    }
    Ok(archive)
}

/// Routing is identical for manifest formats 2.7 and 2.8.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TierFormat {
    V2,
    V3,
}

/// Encoded inner archives in their required encryption state.
///
/// Names ending in `sealed` require backend-encrypted bytes. This structure does
/// not validate encryption. The face-IR/thermal archive is the version-dependent
/// exception described on its field.
pub(crate) struct BiometricArchives<'a> {
    pub iris_sealed: &'a [u8],
    pub normalized_iris_sealed: &'a [u8],
    pub face_sealed: &'a [u8],
    pub fraud_sealed: Option<&'a [u8]>,
    /// Backend-tier2-encrypted tar for V2; plain tar for V3.
    pub face_ir_and_thermal: &'a [u8],
}

/// The complete set of biometric entries, including already-serialized payloads.
///
/// Empty protobuf bytes remain present as empty files; absence of the whole
/// bundle is the only way to omit all biometric entries.
pub(crate) struct PreparedBiometricFiles<'a> {
    pub archives: BiometricArchives<'a>,
    pub face_embeddings_json: &'a [u8],
    pub iris_codes_json: &'a [u8],
    pub iris_code_shares_json: [&'a [u8]; 3],
    pub di_iris_embeddings_pb: &'a [u8],
    pub di_iris_embeddings_shares_pb: [&'a [u8]; 3],
}

/// Pre-encoded files present in tier 0 whether or not biometric entries are included.
/// The signature must correspond to the exact `hashes_json` bytes; this is not checked here.
pub(crate) struct Tier0Files<'a> {
    pub info_json: &'a [u8],
    pub hashes_json: &'a [u8],
    pub hashes_signature: &'a [u8],
    pub backend_keys_json: &'a [u8],
}

pub(crate) struct AuxiliaryTiers {
    pub tier1: Vec<u8>,
    pub tier2: Vec<u8>,
}

/// Encodes tiers 1 and 2 before their compression/encryption and manifest hashing.
///
/// V2 always produces two empty tar archives. V3 places biometric archives in
/// these tiers, or produces empty archives when `biometrics` is `None`.
/// For V3, compress and user-encrypt these outputs before computing the tier
/// digests supplied to [`crate::manifest::ManifestFormat::V3_0`].
pub(crate) fn auxiliary_tiers(
    format: TierFormat,
    timestamp: u64,
    biometrics: Option<&PreparedBiometricFiles<'_>>,
) -> Result<AuxiliaryTiers, ArchiveError> {
    let (tier1, tier2) = match (format, biometrics) {
        (TierFormat::V3, Some(biometrics)) => (
            main_archives(&biometrics.archives),
            vec![(
                "face_ir_and_thermal.tar",
                biometrics.archives.face_ir_and_thermal,
            )],
        ),
        _ => (Vec::new(), Vec::new()),
    };
    Ok(AuxiliaryTiers {
        tier1: encode_tar(timestamp, tier1)?,
        tier2: encode_tar(timestamp, tier2)?,
    })
}

/// Encodes tier 0 after manifest construction and signing.
///
/// Use the same format, timestamp and biometric bundle as for [`auxiliary_tiers`].
/// V2 includes the inner archives here; V3 does not. Both include biometric
/// JSON/protobuf entries only when `biometrics` is `Some`. The result still needs
/// gzip compression and user encryption; it is not a deliverable PCP.
pub(crate) fn tier0(
    format: TierFormat,
    timestamp: u64,
    files: Tier0Files<'_>,
    biometrics: Option<&PreparedBiometricFiles<'_>>,
) -> Result<Vec<u8>, ArchiveError> {
    let mut entries = Vec::new();
    if let (TierFormat::V2, Some(biometrics)) = (format, biometrics) {
        entries.extend(main_archives(&biometrics.archives));
        entries.push((
            "face_ir_and_thermal.tar",
            biometrics.archives.face_ir_and_thermal,
        ));
    }
    entries.push(("info.json", files.info_json));
    if let Some(biometrics) = biometrics {
        entries.push(("face_embeddings.json", biometrics.face_embeddings_json));
        entries.push(("iris_codes.json", biometrics.iris_codes_json));
        entries.extend(
            [
                "iris_code_shares_0.json",
                "iris_code_shares_1.json",
                "iris_code_shares_2.json",
            ]
            .into_iter()
            .zip(biometrics.iris_code_shares_json),
        );
        entries.push(("di_iris_embeddings.pb", biometrics.di_iris_embeddings_pb));
        entries.extend(
            [
                "di_iris_embeddings_shares_0.pb",
                "di_iris_embeddings_shares_1.pb",
                "di_iris_embeddings_shares_2.pb",
            ]
            .into_iter()
            .zip(biometrics.di_iris_embeddings_shares_pb),
        );
    }
    entries.extend([
        ("hashes.sign", files.hashes_signature),
        ("hashes.json", files.hashes_json),
        ("backend_keys.json", files.backend_keys_json),
    ]);
    encode_tar(timestamp, entries)
}

fn main_archives<'a>(
    archives: &BiometricArchives<'a>,
) -> Vec<(&'static str, &'a [u8])> {
    let mut entries = vec![
        ("iris.tar", archives.iris_sealed),
        ("normalized_iris.tar", archives.normalized_iris_sealed),
        ("face.tar", archives.face_sealed),
    ];
    if let Some(fraud) = archives.fraud_sealed {
        entries.push(("fraud.tar", fraud));
    }
    entries
}

#[cfg(test)]
#[path = "../tests/unit/archive.rs"]
mod tests;
