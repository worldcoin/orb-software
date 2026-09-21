//! Personal Custody Package construction primitives.
//!
//! Provides JSON payload, archive, tier-layout and hash-manifest encoding,
//! not a complete package builder or an untrusted-package verifier.

#![forbid(unsafe_code)]

pub mod archive;
pub mod layout;
pub mod manifest;
pub mod payload;
