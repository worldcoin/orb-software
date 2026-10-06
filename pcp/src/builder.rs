//! Synchronous PCP construction from portable inputs.
//!
//! PNG encoding, quantization, secret sharing and signer/recipient authorization
//! belong to the caller. Generated Hyrax commitments use the legacy algorithm;
//! imported commitments and partial-data wire semantics are not supported here.
//! This prototype is not an untrusted-package verifier. Call from a blocking
//! worker in async applications. Plaintext intermediates are not all zeroized.

use orb_pcp_defs::{prost::Message, v1};
use rand::{CryptoRng, RngCore};
use zeroize::Zeroizing;

use crate::{archive, crypto, manifest, metadata, payload};
use manifest::sha256;

/// Coupled archive placement, inner protection and manifest tier coverage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Envelope {
    V2,
    V3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PcpVersion {
    V2_7,
    V2_8,
    V3_0,
}

impl PcpVersion {
    fn validate_input<E>(
        self,
        request: &BuildRequest<'_>,
    ) -> Result<(), BuildError<E>> {
        match self {
            Self::V2_7 => {
                if request.info.device_public_key.is_some() {
                    return Err(BuildError::DeviceKeyVersionMismatch);
                }
            }
            Self::V2_8 => {
                if request.info.device_public_key.is_none() {
                    return Err(BuildError::DeviceKeyVersionMismatch);
                }
            }
            Self::V3_0 => {}
        }
        Ok(())
    }

    fn label(self) -> &'static str {
        match self {
            Self::V2_7 => "2.7",
            Self::V2_8 => "2.8",
            Self::V3_0 => "3.0",
        }
    }

    fn envelope(self) -> Envelope {
        match self {
            Self::V2_7 | Self::V2_8 => Envelope::V2,
            Self::V3_0 => Envelope::V3,
        }
    }
}

/// One privacy decision controls files, manifest hashes and metadata image IDs.
pub enum BiometricPolicy<'a> {
    Redacted,
    /// Consumer-prepared data. Shares are not checked against codes/embeddings.
    /// Hyrax commitments are generated from the supplied normalized bytes;
    /// imported commitments are not accepted.
    Included {
        images: &'a archive::PackageImages<'a>,
        face_embeddings: &'a [v1::FaceEmbedding],
        iris_codes: &'a v1::IrisCodes,
        /// Same-index files belong to the same recipient.
        iris_code_shares: &'a [v1::IrisCodeShares; 3],
        /// Default messages produce the empty DI files of a capture without DI data.
        di_embeddings: &'a v1::DiIrisEmbeddings,
        di_embedding_shares: &'a [v1::DiIrisEmbeddingShares; 3],
    },
}

pub struct BuildRequest<'a> {
    pub version: PcpVersion,
    /// Whole Unix seconds used for archive and gzip headers.
    pub timestamp: u64,
    /// Written as `info.json` as given, except for the builder-owned salts,
    /// multiframe image ID lists and, under redaction, all image IDs. Absent
    /// fields are omitted.
    pub info: &'a v1::Info,
    pub user_public_key: &'a [u8; 32],
    /// Normal builds serialize these same keys and use them for inner encryption.
    pub backend_keys: payload::BackendKeys<'a>,
    pub biometrics: BiometricPolicy<'a>,
    /// Written verbatim as binary `migration.pb` after `info.json` and hashed in
    /// `hashes.json`. Set by TEE migrations; Orb captures leave it `None`.
    pub migration: Option<&'a v1::Migration>,
}

/// Final encrypted tiers and SHA-256 checksums of those ciphertext bytes.
pub struct Package {
    pub tier0: Vec<u8>,
    pub tier1: Vec<u8>,
    pub tier2: Vec<u8>,
    pub tier0_checksum: [u8; 32],
    pub tier1_checksum: [u8; 32],
    pub tier2_checksum: [u8; 32],
}

/// Unencrypted diagnostic gzip tiers. Never upload these as a PCP.
///
/// Inner archives are also plaintext and may contain biometrics, shares and
/// Hyrax blinding factors. Even redacted output retains identity metadata.
/// The caller must protect and clear these buffers; they are not zeroized on drop.
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
    #[error("device key presence does not match the requested PCP version")]
    DeviceKeyVersionMismatch,
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

/// Builds all three tiers, invoking the supplied raw-digest signer once.
/// No successful package is returned on failure. The signer owns its retry and
/// deadline policy. V2.7 requires no device key; V2.8 requires one; V3 accepts
/// either. Version negotiation remains a consumer responsibility.
/// This function always encrypts, including when diagnostic features are enabled.
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

/// Builds sensitive, unencrypted diagnostics, never a deliverable PCP.
///
/// Requires the explicitly enabled `not-prod-diagnostics` feature. Both inner
/// sealing and outer sealing are skipped, but hashing, signing and gzip remain.
/// V3 tier hashes cover these diagnostic gzip bytes, not encrypted tier bytes.
/// Backend keys are still serialized; recipient keys are not validated or used
/// for encryption.
/// This performs no filesystem writes. Never upload, publish or log the output.
/// Enabling this feature does not change the encrypted behavior of [`build`].
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
    request.version.validate_input(request)?;
    if request.timestamp > u64::from(u32::MAX) {
        return Err(archive::ArchiveError::TimestampOutOfRange.into());
    }
    let envelope = request.version.envelope();
    let backend_keys_json = payload::backend_keys(&request.backend_keys)?;
    let mut hashes = vec![("backend_keys.json".to_owned(), sha256(&backend_keys_json))];
    let migration_pb = request.migration.map(Message::encode_to_vec);
    if let Some(migration_pb) = &migration_pb {
        hashes.push(("migration.pb".to_owned(), sha256(migration_pb)));
    }
    let prepared = envelope.prepare_biometrics(request, rng, seal)?;
    if let Some(prepared) = &prepared {
        hashes.extend(prepared.archives.hashes.iter().cloned());
    }
    let biometric_files = prepared.as_ref().map(Prepared::layout);
    let auxiliary = archive::auxiliary_tiers(
        envelope,
        request.timestamp,
        biometric_files.as_ref(),
    )?;
    let tier1 = seal_tier(&auxiliary.tier1, request, "tier1.tar.gz", seal)?;
    let tier2 = seal_tier(&auxiliary.tier2, request, "tier2.tar.gz", seal)?;
    let tier1_checksum = sha256(&tier1);
    let tier2_checksum = sha256(&tier2);

    let info = encode_metadata(request, rng)?;
    for (name, hash) in info.hashes {
        hashes.push((name.to_owned(), hash));
    }
    let signed = manifest::encode_and_sign(
        request.version.label(),
        envelope.manifest_tier_entries([tier1_checksum, tier2_checksum]),
        hashes.iter().map(|(name, hash)| (name.as_str(), *hash)),
        sign_digest,
    )?;
    let tier0 = archive::tier0(
        envelope,
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
                face_ir_and_thermal: &self.archives.face_ir_and_thermal,
            },
            face_embeddings_json: &self.face_embeddings,
            daugman: &self.daugman,
            di: &self.di,
        }
    }
}

impl Envelope {
    fn manifest_tier_entries(
        self,
        [tier_1, tier_2]: [[u8; 32]; 2],
    ) -> manifest::TierEntries {
        match self {
            Self::V2 => manifest::TierEntries::None,
            Self::V3 => manifest::TierEntries::Present { tier_1, tier_2 },
        }
    }

    fn prepare_biometrics<E>(
        self,
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
            .map(|tar| {
                seal(tar, request.backend_keys.face.public_key).map(Zeroizing::new)
            })
            .transpose()?;
        match self {
            Self::V2 => {
                archives.face_ir_and_thermal = Zeroizing::new(seal(
                    &archives.face_ir_and_thermal,
                    request.backend_keys.tier2.public_key,
                )?);
            }
            Self::V3 => {}
        }
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

fn multiframe_ids<'a>(eye: &Option<archive::IrisEye<'a>>) -> Vec<&'a str> {
    eye.iter()
        .flat_map(|eye| eye.multiframe)
        .map(|frame| frame.image_id)
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
