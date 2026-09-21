//! Personal Custody Package construction primitives.
//!
//! Provides JSON/protobuf payload, archive, tier-layout and hash-manifest encoding
//! plus digest signing and sealed-box encryption, not a complete builder or an
//! untrusted-package verifier.
//!
//! Building requires `protoc` for the `orb-pcp-defs` schemas and libsodium
//! discoverable through `pkg-config`.

#![forbid(unsafe_code)]

pub mod archive;
pub mod di;
pub mod encryption;
pub mod layout;
pub mod manifest;
pub mod payload;
