//! Portable Personal Custody Package construction.
//!
//! [`builder`] composes payload encoding, legacy Hyrax generation, tier layout,
//! digest signing and sealed-box encryption into a construction prototype.
//! Consumer integration and untrusted-package verification remain separate work.
//!
//! Building requires `protoc` for the `orb-pcp-defs` schemas and libsodium
//! discoverable through `pkg-config`.

#![forbid(unsafe_code)]

pub mod archive;
pub mod builder;
pub mod commitment;
pub mod di;
pub mod encryption;
pub mod inner;
pub mod layout;
pub mod manifest;
pub mod metadata;
pub mod payload;
