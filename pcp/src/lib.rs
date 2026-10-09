//! Personal Custody Package construction.
//!
//! [build] assembles, hashes, signs and encrypts caller-prepared inputs: encoded
//! PNGs, the shared [v1] messages for metadata and biometric payloads, recipient
//! keys and a callback signing the raw SHA-256 manifest digest. Construction is
//! synchronous; call it from a blocking worker in async applications.
//! Zeroization of intermediate buffers is best-effort.
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
pub use builder::{build, BiometricPolicy, BuildError, BuildRequest, Package};
#[cfg(feature = "not-prod-diagnostics")]
pub use builder::{build_unencrypted_for_diagnostics, DiagnosticPackage};
pub use crypto::{CommitmentError, SealingError};
pub use manifest::{ManifestError, SigningError, PCP_VERSION};
pub use metadata::MetadataError;
/// The shared PCP schema used for `info.json`, the payload files and `migration.pb`.
pub use orb_pcp_defs::v1;
/// Image IDs of multiframe captures, which name their files in the package.
pub use orb_wld_data_id::ImageId;
pub use payload::{BackendKey, BackendKeys};
