//! Portable Personal Custody Package construction.
//!
//! [build] assembles, hashes, signs and encrypts consumer-prepared inputs.
//! Callers supply encoded PNGs, biometric values/shares, authorized recipient
//! keys and a callback signing the raw SHA-256 digest. Call from a blocking
//! worker in async applications. This is not an untrusted-package verifier.
//! Plaintext intermediates are not all automatically zeroized.
//!
//! Building requires protoc and libsodium discoverable through pkg-config.

#![forbid(unsafe_code)]

mod archive;
mod builder;
mod crypto;
mod manifest;
mod metadata;
mod payload;

pub use archive::{
    ArchiveError, FraudImages, InnerArchiveError, IrisEye, IrisFrame,
    NormalizedIrisFrame, PackageImages,
};
pub use builder::{
    build, BiometricData, BiometricPolicy, BuildError, BuildRequest, IrisShares,
    Package, PcpVersion,
};
pub use crypto::{CommitmentError, SealingError};
pub use manifest::{ManifestError, SigningError};
pub use metadata::{MetadataError, PackageInfo};
pub use payload::{
    BackendKey, BackendKeys, DiEncodingError, DiEye, FaceEmbedding, IrisCodes,
};
