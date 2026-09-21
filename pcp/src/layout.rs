//! Outer-tier layout for already-prepared PCP payloads.
//!
//! This module owns filenames, ordering and tier placement only. It does not
//! encode or inspect JSON/protobuf contents, encrypt archives, compute hashes,
//! sign manifests, or verify Hyrax commitments. The higher-level builder must
//! establish those invariants before using these primitives.
//!
//! Omitting biometric entries does not redact opaque metadata: the caller must
//! also remove biometric image IDs and manifest entries. Outputs are uncompressed,
//! unencrypted outer tar archives and are not automatically zeroized.

use crate::archive;

/// Routing is identical for manifest formats 2.7 and 2.8.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    V2,
    V3,
}

/// Encoded inner archives in their required encryption state.
///
/// Names ending in `sealed` require backend-encrypted bytes. This structure does
/// not validate encryption. The face-IR/thermal archive is the version-dependent
/// exception described on its field.
pub struct BiometricArchives<'a> {
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
pub struct Biometrics<'a> {
    pub archives: BiometricArchives<'a>,
    pub face_embeddings_json: &'a [u8],
    pub iris_codes_json: &'a [u8],
    pub iris_code_shares_json: [&'a [u8]; 3],
    pub di_iris_embeddings_pb: &'a [u8],
    pub di_iris_embeddings_shares_pb: [&'a [u8]; 3],
}

/// Pre-encoded files present in tier 0 whether or not biometric entries are included.
/// The signature must correspond to the exact `hashes_json` bytes; this is not checked here.
pub struct Tier0Files<'a> {
    pub info_json: &'a [u8],
    pub hashes_json: &'a [u8],
    pub hashes_signature: &'a [u8],
    pub backend_keys_json: &'a [u8],
}

pub struct AuxiliaryTiers {
    pub tier1: Vec<u8>,
    pub tier2: Vec<u8>,
}

/// Encodes tiers 1 and 2 before their compression/encryption and manifest hashing.
///
/// V2 always produces two empty tar archives. V3 places biometric archives in
/// these tiers, or produces empty archives when `biometrics` is `None`.
/// For V3, compress and user-encrypt these outputs before computing the tier
/// digests supplied to [`crate::manifest::Version::V3_0`].
pub fn auxiliary_tiers(
    format: Format,
    timestamp: u64,
    biometrics: Option<&Biometrics<'_>>,
) -> Result<AuxiliaryTiers, archive::Error> {
    let (tier1, tier2) = match (format, biometrics) {
        (Format::V3, Some(biometrics)) => (
            main_archives(&biometrics.archives),
            vec![(
                "face_ir_and_thermal.tar",
                biometrics.archives.face_ir_and_thermal,
            )],
        ),
        _ => (Vec::new(), Vec::new()),
    };
    Ok(AuxiliaryTiers {
        tier1: archive::encode_tar(timestamp, tier1)?,
        tier2: archive::encode_tar(timestamp, tier2)?,
    })
}

/// Encodes tier 0 after manifest construction and signing.
///
/// Use the same format, timestamp and biometric bundle as for [`auxiliary_tiers`].
/// V2 includes the inner archives here; V3 does not. Both include biometric
/// JSON/protobuf entries only when `biometrics` is `Some`. The result still needs
/// gzip compression and user encryption; it is not a deliverable PCP.
pub fn tier0(
    format: Format,
    timestamp: u64,
    files: Tier0Files<'_>,
    biometrics: Option<&Biometrics<'_>>,
) -> Result<Vec<u8>, archive::Error> {
    let mut entries = Vec::new();
    if let (Format::V2, Some(biometrics)) = (format, biometrics) {
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
    archive::encode_tar(timestamp, entries)
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
