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
    Included(Biometrics<'a>),
}

/// Accepted messages are written as given. Hyrax commitments are generated from the
/// normalized images.
///
/// The iris and DI groups are independently optional. An omitted group contributes
/// no files or hashes; an included group contributes its present data and share
/// files. Share array indices identify recipients.
///
/// Supplied Iris data and version consistency are validated during construction.
/// For a supplied Iris group, strict mode additionally requires codes, all three
/// shares, and their versions.
/// [`Di`] validates its fields and versions during construction.
#[derive(bon::Builder)]
#[builder(start_fn = images)]
#[builder(finish_fn = face_embeddings)]
pub struct Biometrics<'a> {
    #[builder(start_fn)]
    images: &'a archive::PackageImages<'a>,
    #[builder(finish_fn)]
    face_embeddings: &'a [v1::FaceEmbedding],
    iris: Option<Iris<'a>>,
    di: Option<Di<'a>>,
}

pub struct Iris<'a> {
    iris_codes: Option<&'a v1::IrisCodes>,
    iris_code_shares: [Option<&'a v1::IrisCodeShares>; 3],
}

#[bon::bon]
impl<'a> Iris<'a> {
    #[builder(start_fn = codes)]
    #[builder(finish_fn = shares)]
    pub fn new(
        #[builder(start_fn)] iris_codes: Option<&'a v1::IrisCodes>,
        #[builder(finish_fn)] iris_code_shares: [Option<&'a v1::IrisCodeShares>; 3],
    ) -> Result<Self, BiometricValidationError> {
        if let Some(codes) = iris_codes {
            let valid = [
                &codes.left_iris_code,
                &codes.right_iris_code,
                &codes.left_mask_code,
                &codes.right_mask_code,
            ]
            .iter()
            .all(|value| value.is_some());

            if !valid {
                return Err("One of the values in v1::IrisCodes is missing.".into());
            }
        }

        for share in iris_code_shares.iter().flatten() {
            let valid = [
                &share.left_iris_code_shares,
                &share.left_mask_code_shares,
                &share.right_iris_code_shares,
                &share.right_mask_code_shares,
            ]
            .iter()
            .all(|value| value.is_some());

            if !valid {
                return Err(
                    "One of the values in v1::IrisCodeShares is missing.".into()
                );
            }

            if let Some(codes) = iris_codes
                && let (Some(code_version), Some(share_version)) =
                    (&codes.iris_version, &share.iris_version)
                && share_version != code_version
            {
                return Err(
                    "Share iris version does not match iris code version.".into()
                );
            }
        }

        Ok(Self {
            iris_codes,
            iris_code_shares,
        })
    }

    pub(crate) fn validate(&self) -> Result<(), BiometricValidationError> {
        let Some(codes) = self.iris_codes else {
            return Err("Iris codes are missing.".into());
        };

        if codes.iris_version.is_none() {
            return Err("Iris code version is missing.".into());
        }

        for share in &self.iris_code_shares {
            let Some(share) = share else {
                return Err("One or more iris code shares are missing.".into());
            };

            if share.iris_version.is_none() {
                return Err("Iris version is missing from a share.".into());
            }

            if share.iris_shares_version.is_none() {
                return Err("Iris shares version is missing.".into());
            }
        }

        Ok(())
    }
}

pub struct Di<'a> {
    di_embeddings: &'a v1::DiIrisEmbeddings,
    di_embedding_shares: [&'a v1::DiIrisEmbeddingShares; 3],
}

#[bon::bon]
impl<'a> Di<'a> {
    #[builder(start_fn = embeddings)]
    #[builder(finish_fn = shares)]
    pub fn new(
        #[builder(start_fn)] di_embeddings: &'a v1::DiIrisEmbeddings,
        #[builder(finish_fn)] di_embedding_shares: [&'a v1::DiIrisEmbeddingShares; 3],
    ) -> Result<Self, BiometricValidationError> {
        let Some(embedding) = &di_embeddings.embedding_v1 else {
            return Err(
                "At least one of the fields in v1::DiIrisEmbeddings must be present"
                    .into(),
            );
        };

        for share in di_embedding_shares {
            let Some(share) = &share.share_v1 else {
                return Err(
                    "At least one of the fields in each v1::DiIrisEmbeddingsShares must be present"
                        .into(),
                );
            };

            if share.model_version != embedding.model_version {
                return Err(
                    "Share model version does not match embedding model version."
                        .into(),
                );
            }

            if share.embedding_version != embedding.embedding_version {
                return Err(
                    "Share embedding version does not match embedding version.".into(),
                );
            }
        }

        Ok(Self {
            di_embeddings,
            di_embedding_shares,
        })
    }
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
#[error("invalid biometrics: {0}")]
pub struct BiometricValidationError(pub String);

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
    #[error(transparent)]
    InvalidBiometrics(#[from] BiometricValidationError),
}

impl From<&str> for BiometricValidationError {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

#[derive(bon::Builder)]
#[builder(start_fn = timestamp)]
#[builder(finish_fn = signer)]
pub struct Builder<'a, Rng, DigestSigner, E>
where
    Rng: RngCore + CryptoRng,
    DigestSigner: FnOnce(&[u8; 32]) -> Result<Vec<u8>, E>,
{
    /// Whole Unix seconds used for archive and gzip headers.
    #[builder(start_fn)]
    timestamp: u32,
    #[builder(finish_fn)]
    digest_signer: DigestSigner,
    /// Written as `info.json` as given, except for the builder-owned salts,
    /// multiframe image ID lists and, under redaction, all image IDs. Absent
    /// fields are omitted.
    info: &'a v1::Info,
    user_public_key: &'a [u8; 32],
    /// Written to `backend_keys.json` and used to encrypt the inner archives.
    backend_keys: payload::BackendKeys<'a>,
    biometrics: BiometricPolicy<'a>,
    /// Check completeness of a supplied Iris group; the group itself is optional.
    /// Defaults to true. Disabling strictness is only for backward compatibility
    /// with old PCP values that may omit records or versions within that group.
    #[builder(default = true)]
    strict: bool,
    /// Written as binary `migration.pb` after `info.json` and hashed in
    /// `hashes.json`. Set by TEE migrations.
    migration: Option<&'a v1::Migration>,
    rng: Rng,
}

impl<'a, Rng, DigestSigner, E> Builder<'a, Rng, DigestSigner, E>
where
    Rng: RngCore + CryptoRng,
    DigestSigner: FnOnce(&[u8; 32]) -> Result<Vec<u8>, E>,
{
    /// Builds the encrypted package, calling `sign_digest` once with the raw SHA-256
    /// digest of `hashes.json`. The signer owns its retry and deadline policy.
    pub fn build(self) -> Result<Package, BuildError<E>> {
        let BuiltTiers {
            tiers: [tier0, tier1, tier2],
            checksums: [tier0_checksum, tier1_checksum, tier2_checksum],
        } = self.build_with_sealer(crypto::seal)?;

        Ok(Package {
            tier0,
            tier1,
            tier2,
            tier0_checksum,
            tier1_checksum,
            tier2_checksum,
        })
    }

    /// Builds the package like [`Self::build`] but skips inner and outer sealing, for
    /// local diagnostics. Never upload, publish or log the output.
    #[cfg(feature = "not-prod-diagnostics")]
    pub fn build_unencrypted(self) -> Result<DiagnosticPackage, BuildError<E>> {
        let BuiltTiers {
            tiers: [tier0, tier1, tier2],
            ..
        } = self.build_with_sealer(|bytes, _| Ok(bytes.to_vec()))?;

        Ok(DiagnosticPackage {
            tier0,
            tier1,
            tier2,
        })
    }

    fn build_with_sealer(mut self, seal: Sealer) -> Result<BuiltTiers, BuildError<E>> {
        if self.strict
            && let BiometricPolicy::Included(Biometrics {
                iris: Some(iris), ..
            }) = &self.biometrics
        {
            iris.validate()?;
        }

        let backend_keys_json = payload::backend_keys(&self.backend_keys)?;
        let mut hashes =
            vec![("backend_keys.json".to_owned(), sha256(&backend_keys_json))];
        let migration_pb = self.migration.map(Message::encode_to_vec);

        if let Some(migration_pb) = &migration_pb {
            hashes.push(("migration.pb".to_owned(), sha256(migration_pb)));
        }

        let prepared = self.prepare_biometrics(seal)?;

        if let Some(prepared) = &prepared {
            hashes.extend(prepared.archives.hashes.iter().cloned());
        }

        let biometric_files = prepared.as_ref().map(Prepared::layout);
        let empty = archive::encode_tar(self.timestamp as u64, std::iter::empty())?;

        let tier1 = seal_tier(
            &empty,
            self.timestamp,
            self.user_public_key,
            "tier1.tar.gz",
            seal,
        )?;

        let tier2 = seal_tier(
            &empty,
            self.timestamp,
            self.user_public_key,
            "tier2.tar.gz",
            seal,
        )?;

        let tier1_checksum = sha256(&tier1);
        let tier2_checksum = sha256(&tier2);

        let info = self.encode_metadata()?;

        for (name, hash) in info.hashes {
            hashes.push((name.to_owned(), hash));
        }

        let signed = manifest::encode_and_sign(
            hashes.iter().map(|(name, hash)| (name.as_str(), *hash)),
            self.digest_signer,
        )?;

        let tier0 = archive::tier0(
            self.timestamp as u64,
            archive::Tier0Files {
                info_json: &info.info_json,
                hashes_json: &signed.hashes_json,
                hashes_signature: &signed.hashes_signature,
                backend_keys_json: &backend_keys_json,
                migration_pb: migration_pb.as_deref(),
            },
            biometric_files.as_ref(),
        )?;

        let tier0 = seal_tier(
            &tier0,
            self.timestamp,
            self.user_public_key,
            "tier0.tar.gz",
            seal,
        )?;

        Ok(BuiltTiers {
            checksums: [sha256(&tier0), tier1_checksum, tier2_checksum],
            tiers: [tier0, tier1, tier2],
        })
    }

    fn prepare_biometrics(
        &mut self,
        seal: Sealer,
    ) -> Result<Option<Prepared>, BuildError<E>> {
        let BiometricPolicy::Included(biometrics) = &self.biometrics else {
            return Ok(None);
        };

        let Biometrics {
            images,
            face_embeddings,
            iris,
            di,
        } = &biometrics;

        let mut archives =
            archive::encode_inner(self.timestamp as u64, images, &mut self.rng)?;

        archives.iris =
            Zeroizing::new(seal(&archives.iris, self.backend_keys.iris.public_key)?);

        archives.normalized_iris = Zeroizing::new(seal(
            &archives.normalized_iris,
            self.backend_keys.normalized_iris.public_key,
        )?);

        archives.face =
            Zeroizing::new(seal(&archives.face, self.backend_keys.face.public_key)?);

        archives.fraud = archives
            .fraud
            .as_deref()
            .map(|tar| seal(tar, self.backend_keys.face.public_key).map(Zeroizing::new))
            .transpose()?;

        archives.face_ir_and_thermal = Zeroizing::new(seal(
            &archives.face_ir_and_thermal,
            self.backend_keys.tier2.public_key,
        )?);

        let face_embeddings = payload::face_embeddings(face_embeddings)?;

        let daugman = iris
            .as_ref()
            .map(|i| payload::encode_daugman(i.iris_codes, i.iris_code_shares))
            .transpose()?;

        let di = di
            .as_ref()
            .map(|di| payload::encode_di(di.di_embeddings, di.di_embedding_shares));

        for (name, bytes) in [
            ("face_embeddings.json", Some(face_embeddings.as_slice())),
            (
                "iris_codes.json",
                daugman.as_ref().and_then(|d| d.codes.as_deref()),
            ),
            (
                "di_iris_embeddings.pb",
                di.as_ref().map(|di| di.embeddings.as_slice()),
            ),
        ] {
            if let Some(bytes) = bytes {
                archives.hashes.push((name.to_owned(), sha256(bytes)));
            }
        }

        for (i, share) in daugman.iter().flat_map(|d| d.shares.iter().enumerate()) {
            if let Some(share) = share {
                archives
                    .hashes
                    .push((format!("iris_code_shares_{i}.json"), sha256(share)));
            }
        }

        for (i, share) in di.iter().flat_map(|di| di.shares.iter().enumerate()) {
            archives
                .hashes
                .push((format!("di_iris_embeddings_shares_{i}.pb"), sha256(share)));
        }

        Ok(Some(Prepared {
            archives,
            face_embeddings,
            daugman,
            di,
        }))
    }

    fn encode_metadata(
        &mut self,
    ) -> Result<metadata::EncodedMetadata, metadata::MetadataError> {
        let images = match &self.biometrics {
            BiometricPolicy::Redacted => metadata::ImageIdPolicy::Redacted,
            BiometricPolicy::Included(Biometrics { images, .. }) => {
                metadata::ImageIdPolicy::Included {
                    left_multiframe: multiframe_ids(&images.left),
                    right_multiframe: multiframe_ids(&images.right),
                }
            }
        };

        metadata::encode(self.info, images, &mut self.rng)
    }
}

fn seal_tier<E>(
    tar: &[u8],
    timestamp: u32,
    user_public_key: &[u8; 32],
    name: &str,
    seal: Sealer,
) -> Result<Vec<u8>, BuildError<E>> {
    let gzip = archive::compress(tar, timestamp as u64, name)?;
    Ok(seal(&gzip, user_public_key)?)
}

struct Prepared {
    archives: archive::InnerArchives,
    face_embeddings: Vec<u8>,
    daugman: Option<payload::EncodedDaugman>,
    di: Option<payload::EncodedDi>,
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
            daugman: self.daugman.as_ref(),
            di: self.di.as_ref(),
        }
    }
}

fn multiframe_ids(eye: &archive::IrisEye<'_>) -> Vec<String> {
    eye.multiframe
        .iter()
        .map(|frame| frame.image_id.to_string())
        .collect()
}
