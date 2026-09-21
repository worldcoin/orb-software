//! Personal Custody Package construction primitives.
//!
//! Provides JSON/protobuf payload, archive, tier-layout and hash-manifest encoding,
//! not a complete package builder or an untrusted-package verifier.
//!
//! Building requires `protoc` for the `orb-pcp-defs` schemas.

#![forbid(unsafe_code)]

pub mod archive;
pub mod di;
pub mod layout;
pub mod manifest;
pub mod payload;
