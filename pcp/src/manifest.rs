//! Encoding of the signed `hashes.json` manifest.

use std::collections::BTreeMap;

use data_encoding::HEXLOWER;

/// The PCP version written to the `version` field of every manifest.
pub const PCP_VERSION: &str = "2.8";

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("duplicate manifest entry")]
    DuplicateEntry,
    #[error("reserved manifest entry")]
    ReservedEntry,
    #[error("could not serialize hash manifest")]
    Serialization(#[from] serde_json::Error),
}

/// Exact manifest and signer output bytes, ready for archive assembly.
pub(crate) struct SignedManifest {
    pub hashes_json: Vec<u8>,
    pub hashes_signature: Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
pub enum SigningError<E> {
    #[error("could not encode the manifest for signing")]
    Manifest(#[from] ManifestError),
    #[error("manifest signer failed")]
    Signer(#[source] E),
}

/// Encodes the manifest and calls `sign_digest` once with its raw SHA-256 digest.
///
/// The callback signs this 32-byte digest without hashing it again, and its
/// bytes become `hashes.sign` unchanged. The signer is called only after
/// encoding succeeds; its errors are returned as [`SigningError::Signer`].
pub(crate) fn encode_and_sign<'a, E>(
    hashes: impl IntoIterator<Item = (&'a str, [u8; 32])>,
    sign_digest: impl FnOnce(&[u8; 32]) -> Result<Vec<u8>, E>,
) -> Result<SignedManifest, SigningError<E>> {
    let hashes_json = encode(hashes)?;
    let hashes_signature =
        sign_digest(&sha256(&hashes_json)).map_err(SigningError::Signer)?;
    Ok(SignedManifest {
        hashes_json,
        hashes_signature,
    })
}

pub(crate) fn sha256(bytes: &[u8]) -> [u8; 32] {
    ring::digest::digest(&ring::digest::SHA256, bytes)
        .as_ref()
        .try_into()
        .expect("SHA-256 produces 32 bytes")
}

/// Encodes named SHA-256 digests and the version as compact JSON with sorted
/// keys. Duplicate names and a `version` entry are rejected. Signatures cover
/// these exact bytes.
pub(crate) fn encode<'a>(
    hashes: impl IntoIterator<Item = (&'a str, [u8; 32])>,
) -> Result<Vec<u8>, ManifestError> {
    let mut fields = BTreeMap::new();
    for (name, digest) in hashes {
        if name == "version" {
            return Err(ManifestError::ReservedEntry);
        }
        if fields.insert(name, HEXLOWER.encode(&digest)).is_some() {
            return Err(ManifestError::DuplicateEntry);
        }
    }
    fields.insert("version", PCP_VERSION.to_owned());
    Ok(serde_json::to_vec(&fields)?)
}

#[cfg(test)]
#[path = "../tests/unit/manifest.rs"]
mod tests;
