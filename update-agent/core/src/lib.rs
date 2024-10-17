#![forbid(unsafe_code)]
#![warn(unreachable_pub)]

mod claim;
pub mod components;
pub mod file_location;
pub mod manifest;
pub mod pubkeys;
mod signatures;
mod slot;
pub mod telemetry;
pub mod version_map;
pub mod versions;

pub use claim::{Claim, ClaimVerificationContext, Source};
pub use components::{Component, Components};
pub use file_location::LocalOrRemote;
pub use manifest::{Manifest, ManifestComponent};
pub use slot::Slot;
pub use version_map::VersionMap;
pub use versions::{Versions, VersionsLegacy};

/// Crates reexported for use
pub mod reexports {
    pub use ed25519_dalek;
}
