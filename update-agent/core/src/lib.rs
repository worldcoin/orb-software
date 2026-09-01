#![forbid(unsafe_code)]
#![warn(unreachable_pub)]

pub mod blockdev;
mod claim;
pub mod components;
pub mod file_location;
pub mod manifest;
pub mod pubkeys;
mod signatures;
mod slot;

pub use claim::{Claim, ClaimVerificationContext, MimeType, Source, UncheckedClaim};
pub use components::{Component, Components};
pub use file_location::LocalOrRemote;
pub use manifest::{Manifest, ManifestComponent};
pub use slot::Slot;

/// Crates reexported for use
pub mod reexports {
    pub use ed25519_dalek;
}
