//! Synchronous PCP construction.
//!
//! PNG encoding, quantization, secret sharing and signer/recipient authorization
//! belong to the caller.

use orb_pcp_defs::{prost::Message, v1};
use rand::{CryptoRng, RngCore};
use zeroize::Zeroizing;

use crate::{archive, crypto, manifest, metadata, payload};
use manifest::sha256;

/// Whether biometric files, their hashes and the image IDs are included.
pub enum BiometricPolicy<'a> {
    Redacted,
    /// Messages are written as given. Hyrax commitments are generated from the
    /// normalized images.
    Included {
        images: &'a archive::PackageImages<'a>,
        face_embeddings: &'a [v1::FaceEmbedding],
        iris_codes: &'a v1::IrisCodes,
        /// Same-index files belong to the same recipient.
        iris_code_shares: &'a [v1::IrisCodeShares; 3],
        /// Default messages produce empty DI files.
        di_embeddings: &'a v1::DiIrisEmbeddings,
        di_embedding_shares: &'a [v1::DiIrisEmbeddingShares; 3],
    },
}

pub struct BuildRequest<'a> {
    /// Whole Unix seconds used for archive and gzip headers.
    pub timestamp: u64,
    /// Written as `info.json` as given, except for the builder-owned salts,
    /// multiframe image ID lists and, under redaction, all image IDs. Absent
    /// fields are omitted.
    pub info: &'a v1::Info,
    pub user_public_key: &'a [u8; 32],
    /// Written to `backend_keys.json` and used to encrypt the inner archives.
    pub backend_keys: payload::BackendKeys<'a>,
    pub biometrics: BiometricPolicy<'a>,
    /// Written as binary `migration.pb` after `info.json` and hashed in
    /// `hashes.json`. Set by TEE migrations.
    pub migration: Option<&'a v1::Migration>,
}

/// Encrypted tiers and SHA-256 checksums of their ciphertext. Tiers 1 and 2 are
/// empty archives.
pub struct Package {
    pub tier0: Vec<u8>,
    pub tier1: Vec<u8>,
    pub tier2: Vec<u8>,
    pub tier0_checksum: [u8; 32],
    pub tier1_checksum: [u8; 32],
    pub tier2_checksum: [u8; 32],
}

/// Plaintext gzip tiers for local diagnostics, including plaintext inner
/// archives with biometrics, shares and Hyrax blinding factors. Never upload,
/// publish or log them; the caller must clear these buffers.
#[cfg(feature = "not-prod-diagnostics")]
pub struct DiagnosticPackage {
    pub tier0: Vec<u8>,
    pub tier1: Vec<u8>,
    pub tier2: Vec<u8>,
}

struct BuiltTiers {
    tiers: [Vec<u8>; 3],
    checksums: [[u8; 32]; 3],
}

type Sealer = fn(&[u8], &[u8; 32]) -> Result<Vec<u8>, crypto::SealingError>;

#[derive(Debug, thiserror::Error)]
pub enum BuildError<E> {
    #[error("metadata construction failed")]
    Metadata(#[from] metadata::MetadataError),
    #[error("inner archive construction failed")]
    Inner(#[from] archive::InnerArchiveError),
    #[error("payload serialization failed")]
    Json(#[from] serde_json::Error),
    #[error("archive encoding failed")]
    Archive(#[from] archive::ArchiveError),
    #[error("encryption failed")]
    Encryption(#[from] crypto::SealingError),
    #[error("manifest signing failed")]
    Signing(#[from] manifest::SigningError<E>),
}

/// Builds the encrypted package, calling `sign_digest` once with the raw SHA-256
/// digest of `hashes.json`. The signer owns its retry and deadline policy.
pub fn build<E>(
    request: &BuildRequest<'_>,
    rng: &mut (impl RngCore + CryptoRng),
    sign_digest: impl FnOnce(&[u8; 32]) -> Result<Vec<u8>, E>,
) -> Result<Package, BuildError<E>> {
    let BuiltTiers {
        tiers: [tier0, tier1, tier2],
        checksums: [tier0_checksum, tier1_checksum, tier2_checksum],
    } = build_with_sealer(request, rng, sign_digest, crypto::seal)?;
    Ok(Package {
        tier0,
        tier1,
        tier2,
        tier0_checksum,
        tier1_checksum,
        tier2_checksum,
    })
}

/// Builds the package like [`build`] but skips inner and outer sealing, for
/// local diagnostics. Never upload, publish or log the output.
#[cfg(feature = "not-prod-diagnostics")]
pub fn build_unencrypted_for_diagnostics<E>(
    request: &BuildRequest<'_>,
    rng: &mut (impl RngCore + CryptoRng),
    sign_digest: impl FnOnce(&[u8; 32]) -> Result<Vec<u8>, E>,
) -> Result<DiagnosticPackage, BuildError<E>> {
    let BuiltTiers {
        tiers: [tier0, tier1, tier2],
        ..
    } = build_with_sealer(request, rng, sign_digest, |bytes, _| Ok(bytes.to_vec()))?;
    Ok(DiagnosticPackage {
        tier0,
        tier1,
        tier2,
    })
}

fn build_with_sealer<E>(
    request: &BuildRequest<'_>,
    rng: &mut (impl RngCore + CryptoRng),
    sign_digest: impl FnOnce(&[u8; 32]) -> Result<Vec<u8>, E>,
    seal: Sealer,
) -> Result<BuiltTiers, BuildError<E>> {
    if request.timestamp > u64::from(u32::MAX) {
        return Err(archive::ArchiveError::TimestampOutOfRange.into());
    }
    let backend_keys_json = payload::backend_keys(&request.backend_keys)?;
    let mut hashes = vec![("backend_keys.json".to_owned(), sha256(&backend_keys_json))];
    let migration_pb = request.migration.map(Message::encode_to_vec);
    if let Some(migration_pb) = &migration_pb {
        hashes.push(("migration.pb".to_owned(), sha256(migration_pb)));
    }
    let prepared = prepare_biometrics(request, rng, seal)?;
    if let Some(prepared) = &prepared {
        hashes.extend(prepared.archives.hashes.iter().cloned());
    }
    let biometric_files = prepared.as_ref().map(Prepared::layout);
    let empty = archive::encode_tar(request.timestamp, std::iter::empty())?;
    let tier1 = seal_tier(&empty, request, "tier1.tar.gz", seal)?;
    let tier2 = seal_tier(&empty, request, "tier2.tar.gz", seal)?;
    let tier1_checksum = sha256(&tier1);
    let tier2_checksum = sha256(&tier2);

    let info = encode_metadata(request, rng)?;
    for (name, hash) in info.hashes {
        hashes.push((name.to_owned(), hash));
    }
    let signed = manifest::encode_and_sign(
        hashes.iter().map(|(name, hash)| (name.as_str(), *hash)),
        sign_digest,
    )?;
    let tier0 = archive::tier0(
        request.timestamp,
        archive::Tier0Files {
            info_json: &info.info_json,
            hashes_json: &signed.hashes_json,
            hashes_signature: &signed.hashes_signature,
            backend_keys_json: &backend_keys_json,
            migration_pb: migration_pb.as_deref(),
        },
        biometric_files.as_ref(),
    )?;
    let tier0 = seal_tier(&tier0, request, "tier0.tar.gz", seal)?;
    Ok(BuiltTiers {
        checksums: [sha256(&tier0), tier1_checksum, tier2_checksum],
        tiers: [tier0, tier1, tier2],
    })
}

struct Prepared {
    archives: archive::InnerArchives,
    face_embeddings: Vec<u8>,
    daugman: payload::EncodedDaugman,
    di: payload::EncodedDi,
}

impl Prepared {
    fn layout(&self) -> archive::PreparedBiometricFiles<'_> {
        archive::PreparedBiometricFiles {
            archives: archive::BiometricArchives {
                iris_sealed: &self.archives.iris,
                normalized_iris_sealed: &self.archives.normalized_iris,
                face_sealed: &self.archives.face,
                fraud_sealed: self.archives.fraud.as_ref().map(|tar| tar.as_slice()),
                face_ir_and_thermal_sealed: &self.archives.face_ir_and_thermal,
            },
            face_embeddings_json: &self.face_embeddings,
            daugman: &self.daugman,
            di: &self.di,
        }
    }
}

fn prepare_biometrics<E>(
    request: &BuildRequest<'_>,
    rng: &mut (impl RngCore + CryptoRng),
    seal: Sealer,
) -> Result<Option<Prepared>, BuildError<E>> {
    let BiometricPolicy::Included {
        images,
        face_embeddings,
        iris_codes,
        iris_code_shares,
        di_embeddings,
        di_embedding_shares,
    } = &request.biometrics
    else {
        return Ok(None);
    };
    let mut archives = archive::encode_inner(request.timestamp, images, rng)?;
    archives.iris =
        Zeroizing::new(seal(&archives.iris, request.backend_keys.iris.public_key)?);
    archives.normalized_iris = Zeroizing::new(seal(
        &archives.normalized_iris,
        request.backend_keys.normalized_iris.public_key,
    )?);
    archives.face =
        Zeroizing::new(seal(&archives.face, request.backend_keys.face.public_key)?);
    archives.fraud = archives
        .fraud
        .as_deref()
        .map(|tar| seal(tar, request.backend_keys.face.public_key).map(Zeroizing::new))
        .transpose()?;
    archives.face_ir_and_thermal = Zeroizing::new(seal(
        &archives.face_ir_and_thermal,
        request.backend_keys.tier2.public_key,
    )?);
    let face_embeddings = payload::face_embeddings(face_embeddings)?;
    let daugman = payload::encode_daugman(iris_codes, iris_code_shares)?;
    let di = payload::encode_di(di_embeddings, di_embedding_shares);
    for (name, bytes) in [
        ("face_embeddings.json", face_embeddings.as_slice()),
        ("iris_codes.json", daugman.codes.as_slice()),
        ("di_iris_embeddings.pb", di.embeddings.as_slice()),
    ] {
        archives.hashes.push((name.to_owned(), sha256(bytes)));
    }
    for (i, share) in daugman.shares.iter().enumerate() {
        archives
            .hashes
            .push((format!("iris_code_shares_{i}.json"), sha256(share)));
        archives.hashes.push((
            format!("di_iris_embeddings_shares_{i}.pb"),
            sha256(&di.shares[i]),
        ));
    }
    Ok(Some(Prepared {
        archives,
        face_embeddings,
        daugman,
        di,
    }))
}

fn encode_metadata(
    request: &BuildRequest<'_>,
    rng: &mut (impl RngCore + CryptoRng),
) -> Result<metadata::EncodedMetadata, metadata::MetadataError> {
    let images = match &request.biometrics {
        BiometricPolicy::Redacted => metadata::ImageIdPolicy::Redacted,
        BiometricPolicy::Included { images, .. } => metadata::ImageIdPolicy::Included {
            left_multiframe: multiframe_ids(&images.left),
            right_multiframe: multiframe_ids(&images.right),
        },
    };
    metadata::encode(request.info, images, rng)
}

fn multiframe_ids(eye: &archive::IrisEye<'_>) -> Vec<String> {
    eye.multiframe
        .iter()
        .map(|frame| frame.image_id.to_string())
        .collect()
}

fn seal_tier<E>(
    tar: &[u8],
    request: &BuildRequest<'_>,
    name: &str,
    seal: Sealer,
) -> Result<Vec<u8>, BuildError<E>> {
    let gzip = archive::compress(tar, request.timestamp, name)?;
    Ok(seal(&gzip, request.user_public_key)?)
}
