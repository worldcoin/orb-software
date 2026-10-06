//! Portable Personal Custody Package construction.
//!
//! [build] assembles, hashes, signs and encrypts consumer-prepared inputs.
//! Callers supply encoded PNGs, the shared [v1] messages for metadata and
//! biometric payloads, authorized recipient keys and a callback signing the raw
//! SHA-256 digest. Call from a blocking worker in async applications. This is
//! not an untrusted-package verifier.
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
    NormalizedIrisFrame, PackageImages, PrimaryIrisFrame,
};
pub use builder::{
    build, BiometricPolicy, BuildError, BuildRequest, Package, PcpVersion,
};
#[cfg(feature = "not-prod-diagnostics")]
pub use builder::{build_unencrypted_for_diagnostics, DiagnosticPackage};
pub use crypto::{CommitmentError, SealingError};
pub use manifest::{ManifestError, SigningError};
pub use metadata::MetadataError;
/// The shared PCP schema used for `info.json`, the payload files and `migration.pb`.
pub use orb_pcp_defs::v1;
pub use payload::{BackendKey, BackendKeys};
