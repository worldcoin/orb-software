//! Personal Custody Package construction primitives.
//!
//! Currently provides archive and hash-manifest encoding, not a complete package builder
//! or an untrusted-package verifier.

#![forbid(unsafe_code)]

pub mod archive;
pub mod manifest;
